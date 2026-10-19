import { cookies } from 'next/headers';
import type { UserRecord } from './authStore';
import { describeUserAgent } from '../userAgent';
import { record as audit, requestMeta } from './audit';
import { DEVICE_COOKIE, DEVICE_COOKIE_MAX_AGE_S, looksLikeDeviceToken, newDeviceToken, recordDevice } from './devices';
import { sendNewDeviceEmail } from './email';
import { clientIp, hit } from './rateLimit';
import { createSession, lookupSession, revokeSessionByToken, SESSION_ABSOLUTE_MS, type Auth, type SessionInfo } from './sessions';

export const SESSION_COOKIE = 'spectra_session';

/**
 * The signed-in account, its session and active workspace for this request, or null. Does a
 * database lookup (see lib/server/sessions.ts). Server-only: route handlers and server components.
 */
export async function getSession(): Promise<Auth | null> {
  const jar = await cookies();
  return lookupSession(jar.get(SESSION_COOKIE)?.value);
}

/**
 * Sign this browser in as `user`: create a session row and set the cookie. Any session the
 * browser already had is revoked (token rotation on every sign-in and privilege change).
 * The browser's device cookie is created if missing and the device remembered; with
 * `notifyNewDevice` (password sign-ins), the owner is emailed when the device is new to the account.
 * Call from a route handler.
 */
export async function startSession(user: UserRecord, req: Request, opts: { notifyNewDevice?: boolean } = {}): Promise<SessionInfo & { newDevice: boolean }> {
  const jar = await cookies();
  const ip = clientIp(req);
  const userAgent = req.headers.get('user-agent');

  let deviceToken = jar.get(DEVICE_COOKIE)?.value;
  if (!looksLikeDeviceToken(deviceToken)) deviceToken = newDeviceToken();
  jar.set(DEVICE_COOKIE, deviceToken, {
    httpOnly: true,
    secure: process.env.NODE_ENV === 'production',
    sameSite: 'lax',
    path: '/',
    maxAge: DEVICE_COOKIE_MAX_AGE_S,
  });
  const device = await recordDevice(user.id, deviceToken, { ip, userAgent });

  const { token, session } = await createSession(user, {
    ip,
    userAgent,
    replacing: jar.get(SESSION_COOKIE)?.value,
    deviceHash: device.hash,
  });
  jar.set(SESSION_COOKIE, token, {
    httpOnly: true,
    secure: process.env.NODE_ENV === 'production',
    sameSite: 'lax',
    path: '/',
    maxAge: Math.floor(SESSION_ABSOLUTE_MS / 1000),
  });

  const newDevice = device.isNew && !device.isFirst;
  if (opts.notifyNewDevice && newDevice) await notifyNewDevice(user, { ip, userAgent, sessionId: session.id, req });
  return { ...session, newDevice };
}

/** Audit + (rate-limited, fire-and-forget) email for a sign-in from a device new to the account. */
async function notifyNewDevice(user: UserRecord, info: { ip: string; userAgent: string | null; sessionId: string; req: Request }) {
  const device = describeUserAgent(info.userAgent);
  await audit({ action: 'auth.login.new_device', actor: user, target: { type: 'session', id: info.sessionId }, details: { device: device.label } }, requestMeta(info.req));
  // At most 5 such emails an hour per account, so a script can't flood the inbox.
  if (!(await hit(`new-device-email:${user.id}`, 5, 60 * 60_000)).ok) return;
  sendNewDeviceEmail({ to: user.email, device: device.label, ip: info.ip, when: new Date() }).catch((err) => console.error('[NEW DEVICE EMAIL ERROR]', err));
}

/** Sign this browser out: revoke its session and clear the cookie. */
export async function endSession(): Promise<void> {
  const jar = await cookies();
  const token = jar.get(SESSION_COOKIE)?.value;
  if (token) await revokeSessionByToken(token);
  jar.delete(SESSION_COOKIE);
}
