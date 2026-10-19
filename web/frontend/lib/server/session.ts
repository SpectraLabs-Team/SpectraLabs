import { cookies } from 'next/headers';
import { createSessionToken, verifySessionToken } from './authStore';

export const SESSION_COOKIE = 'spectra_session';

/** The signed-in user from the HttpOnly session cookie, or null. Server-only. */
export function getSession() {
  const token = cookies().get(SESSION_COOKIE)?.value;
  return token ? verifySessionToken(token) : null;
}

const WEEK = 7 * 24 * 60 * 60;

/** Issue the signed session cookie for a user (call from a route handler). */
export function setSessionCookie(user: { email: string; role: 'admin' | 'user'; name?: string }) {
  cookies().set(SESSION_COOKIE, createSessionToken(user), {
    httpOnly: true,
    secure: process.env.NODE_ENV === 'production',
    sameSite: 'lax',
    path: '/',
    maxAge: WEEK,
  });
}
