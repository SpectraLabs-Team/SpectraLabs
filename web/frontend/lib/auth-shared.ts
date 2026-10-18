export type Role = 'admin' | 'user';
export type AuthUser = { email: string; role: Role };
export type AuthMode = 'login' | 'signup';

export const ADMIN_EMAIL = 'spectralabs.team@gmail.com';
export const MIN_PASSWORD = 8;
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
