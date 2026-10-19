export type Role = 'admin' | 'user';
export type AuthUser = { email: string; role: Role; name: string };
export type AuthMode = 'login' | 'signup' | 'forgot';

export const ADMIN_EMAIL = 'spectralabs.team@gmail.com';
export const MIN_PASSWORD = 8;
export const MIN_NAME = 2;
export const MAX_NAME = 80;
export const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]{2,}$/;

export type PasswordRules = {
  length: boolean;
  letter: boolean;
  number: boolean;
  special: boolean;
};

export function checkPasswordRules(password: string): PasswordRules {
  return {
    length: password.length >= MIN_PASSWORD,
    letter: /[a-zA-Z]/.test(password),
    number: /[0-9]/.test(password),
    special: /[^a-zA-Z0-9]/.test(password),
  };
}

export function isPasswordValid(password: string): boolean {
  const r = checkPasswordRules(password);
  return r.length && r.letter && r.number && r.special;
}

/** Role is derived from the email for the UI preview only; the backend must assign the real role. */
export function roleForEmail(email: string): Role {
  return email.trim().toLowerCase() === ADMIN_EMAIL ? 'admin' : 'user';
}

/** Trimmed, single-spaced display name; '' if it fails the length rule. */
export function cleanName(raw: unknown): string {
  if (typeof raw !== 'string') return '';
  const n = raw.trim().replace(/\s+/g, ' ');
  return n.length >= MIN_NAME && n.length <= MAX_NAME ? n : '';
}

/** Fallback for accounts created before names existed: "alex.vance@x.com" → "Alex Vance". */
export function nameFromEmail(email: string): string {
  const local = email.split('@')[0] || email;
  return local
    .split(/[._\-+]+/)
    .filter(Boolean)
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(' ');
}

export function displayName(u: Pick<AuthUser, 'email'> & { name?: string }): string {
  return u.name?.trim() || nameFromEmail(u.email);
}

export function firstName(u: Pick<AuthUser, 'email'> & { name?: string }): string {
  return displayName(u).split(' ')[0];
}

/** "Amogh R Gowda" → "AG", "Alex" → "AL". */
export function initials(u: Pick<AuthUser, 'email'> & { name?: string }): string {
  const parts = displayName(u).split(' ').filter(Boolean);
  if (parts.length >= 2) return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
  return (parts[0] || '?').slice(0, 2).toUpperCase();
}
