import { query } from './db';
import { record } from './audit';
import { count, hit } from './rateLimit';

/**
 * Spike alerts (M19 task 18). Per-IP and per-account limits stop one attacker; they don't tell
 * anyone when many accounts are being tried at once from many addresses (credential stuffing) or
 * when someone is using the code emails to spam people. So every failed sign-in and every emailed
 * code also counts against one site-wide counter, and when a counter crosses its threshold the
 * team is told: an audit event (`security.alert`), a server log line, an email to ALERT_EMAIL (or
 * every active platform admin) and, if set, a POST to ALERT_WEBHOOK_URL (Slack-compatible).
 *
 * Thresholds: ALERT_FAILED_LOGINS and ALERT_CODE_REQUESTS, as "count/minutes" (default 50/10 and
 * 100/10), or "off". At most one alert per kind per hour; counters live in `rate_limits`, so they
 * hold across restarts and server processes.
 *
 * noteSpike() never throws and never blocks the request it is counting beyond one upsert.
 */

export type SpikeKind = 'failed_login' | 'code_request';
export type Threshold = { max: number; windowMs: number };

const DEFAULTS: Record<SpikeKind, Threshold> = {
  failed_login: { max: 50, windowMs: 10 * 60_000 },
  code_request: { max: 100, windowMs: 10 * 60_000 },
};
const ENV_NAME: Record<SpikeKind, string> = { failed_login: 'ALERT_FAILED_LOGINS', code_request: 'ALERT_CODE_REQUESTS' };
const LABEL: Record<SpikeKind, string> = { failed_login: 'failed sign-ins', code_request: 'emailed code requests' };

export const ALERT_COOLDOWN_MS = 60 * 60_000;

/** The threshold for a kind from the environment; null when turned off. Bad values fall back to the default. */
export function spikeThreshold(kind: SpikeKind, env: Record<string, string | undefined> = process.env): Threshold | null {
  const raw = env[ENV_NAME[kind]]?.trim().toLowerCase();
  if (!raw) return DEFAULTS[kind];
  if (raw === 'off') return null;
  const m = /^(\d{1,6})\s*\/\s*(\d{1,4})\s*(?:m|min)?$/.exec(raw);
  if (!m || Number(m[1]) < 1 || Number(m[2]) < 1) {
    console.warn(`[SECURITY] ${ENV_NAME[kind]}="${raw}" is not "count/minutes" (e.g. 50/10) or "off"; using the default.`);
    return DEFAULTS[kind];
  }
  return { max: Number(m[1]), windowMs: Number(m[2]) * 60_000 };
}

export type SpikeAlert = {
  kind: SpikeKind;
  count: number;
  windowMinutes: number;
  /** Failed sign-ins only: the busiest source addresses and how many accounts were tried. */
  topIps?: { ip: string; count: number }[];
  accounts?: number;
};

export type AlertNotifier = (alert: SpikeAlert, recipients: string[]) => Promise<void>;

/** Default notifier: email (loaded lazily so this module has no mail dependency) plus the webhook. */
const defaultNotifier: AlertNotifier = async (alert, recipients) => {
  const jobs: Promise<unknown>[] = [];
  if (recipients.length) {
    jobs.push(import('./email').then((m) => m.sendSecurityAlertEmail({ to: recipients, summary: summarize(alert), alert })));
  }
  const hook = process.env.ALERT_WEBHOOK_URL?.trim();
  if (hook) {
    jobs.push(
      fetch(hook, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ text: `:rotating_light: Spectra Labs security alert: ${summarize(alert)}` }),
        signal: AbortSignal.timeout(5000),
      }).then((r) => {
        if (!r.ok) throw new Error(`webhook answered ${r.status}`);
      }),
    );
  }
  const failed = (await Promise.allSettled(jobs)).filter((r): r is PromiseRejectedResult => r.status === 'rejected');
  for (const f of failed) console.error('[SECURITY ALERT] notification failed:', f.reason instanceof Error ? f.reason.message : f.reason);
};

let notifier: AlertNotifier = defaultNotifier;

/** Replace how alerts are delivered (tests); pass null to restore the default. */
export function setAlertNotifier(fn: AlertNotifier | null): void {
  notifier = fn ?? defaultNotifier;
}

export function summarize(a: SpikeAlert): string {
  let s = `${a.count} ${LABEL[a.kind]} in the last ${a.windowMinutes} minutes (site-wide).`;
  if (a.accounts !== undefined) s += ` ${a.accounts} account${a.accounts === 1 ? '' : 's'} targeted.`;
  if (a.topIps?.length) s += ` Top sources: ${a.topIps.map((t) => `${t.ip} (${t.count})`).join(', ')}.`;
  return s;
}

/** Who gets the email: ALERT_EMAIL (comma-separated) if set, else every active platform admin. */
export async function alertRecipients(env: Record<string, string | undefined> = process.env): Promise<string[]> {
  const listed = (env.ALERT_EMAIL ?? '')
    .split(',')
    .map((e) => e.trim().toLowerCase())
    .filter((e) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(e));
  if (listed.length) return [...new Set(listed)];
  const rows = await query<{ email: string }>(`SELECT email FROM users WHERE platform_role = 'admin' AND status = 'active' ORDER BY email`);
  return rows.map((r) => r.email);
}

/**
 * Count one event. Returns the alert when this event pushed the counter over its threshold and no
 * alert of this kind went out in the last hour; otherwise null.
 */
export async function noteSpike(kind: SpikeKind): Promise<SpikeAlert | null> {
  try {
    const t = spikeThreshold(kind);
    if (!t) return null;
    const n = await count(`spike:${kind}`, t.windowMs);
    if (n < t.max) return null;
    // One alert per kind per cooldown, even with many servers crossing the line together.
    if (!(await hit(`spike:${kind}:alerted`, 1, ALERT_COOLDOWN_MS)).ok) return null;

    const alert: SpikeAlert = { kind, count: n, windowMinutes: Math.round(t.windowMs / 60_000) };
    if (kind === 'failed_login') {
      const since = `${Math.round(t.windowMs / 1000)} seconds`;
      const top = await query<{ ip: string | null; n: number }>(
        `SELECT ip, count(*)::int AS n FROM audit_events
          WHERE action = 'auth.login.failure' AND occurred_at > now() - $1::interval
          GROUP BY ip ORDER BY n DESC, ip LIMIT 5`,
        [since],
      );
      const [{ accounts }] = await query<{ accounts: number }>(
        `SELECT count(DISTINCT coalesce(actor_id::text, actor_email))::int AS accounts FROM audit_events
          WHERE action = 'auth.login.failure' AND occurred_at > now() - $1::interval`,
        [since],
      );
      alert.topIps = top.map((r) => ({ ip: r.ip ?? 'unknown', count: r.n }));
      alert.accounts = accounts;
    }

    console.warn(`[SECURITY ALERT] ${summarize(alert)}`);
    await record({ action: 'security.alert', outcome: 'failure', details: { ...alert } });
    const recipients = await alertRecipients().catch(() => []);
    await notifier(alert, recipients).catch((err) => console.error('[SECURITY ALERT] notification failed:', err instanceof Error ? err.message : err));
    return alert;
  } catch (err) {
    console.error(`[SECURITY] spike counter (${kind}) failed:`, err instanceof Error ? err.message : err);
    return null;
  }
}
