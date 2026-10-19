import crypto from 'crypto';
import fs from 'fs';
import path from 'path';

export const ADMIN_EMAIL = (process.env.ADMIN_EMAIL || 'spectralabs.team@gmail.com').toLowerCase();

// ── AUTH_SECRET: required, no fallback ──
// Keep these rules in step with lib/sessionEdge.ts (Edge middleware) and next.config.js (startup check).
const MIN_AUTH_SECRET_LENGTH = 32;
/** Former hard-coded development default; it is public, so it must never sign anything. */
const RETIRED_SECRETS = new Set(['spectralabs_jwt_default_secret_key_32bytes_required']);

/** The session-signing secret. Throws if AUTH_SECRET is missing, short or a retired default. */
function authSecret(): string {
  const s = process.env.AUTH_SECRET;
  let problem: string | null = null;
  if (!s || !s.trim()) problem = 'AUTH_SECRET is not set';
  else if (s.length < MIN_AUTH_SECRET_LENGTH) problem = `AUTH_SECRET is too short (need ${MIN_AUTH_SECRET_LENGTH}+ characters)`;
  else if (RETIRED_SECRETS.has(s)) problem = 'AUTH_SECRET is the old public development default';
  if (problem) throw new Error(`${problem}. Run \`npm run setup\` in web/frontend to generate one.`);
  return s as string;
}

export type UserRecord = {
  email: string;
  name?: string;
  passwordHash: string;
  role: 'admin' | 'user';
  createdAt: number;
  /** ms epoch of the last password reset; sessions issued before it are rejected. */
  passwordChangedAt?: number;
  /** Ask for an emailed code at every login (in addition to the password). */
  twoFactor?: boolean;
  /** ms epoch of "sign out everywhere"; sessions issued before it are rejected. */
  sessionsRevokedAt?: number;
};

// ── In-Memory OTP Store (5-minute TTL) ──
type PendingOtp = {
  /** SHA-256 of the 6-digit code; the code itself is never stored. */
  codeHash: string;
  passwordHash: string;
  name: string;
  /** Sign-up only: the 2FA choice made on the form, applied when the account is created. */
  twoFactor?: boolean;
  expiresAt: number;
  attempts: number;
};

// Global maps survive hot module reloading in development.
const globalForAuth = global as unknown as {
  __spectra_otps?: Map<string, PendingOtp>;
  __spectra_users?: Map<string, UserRecord>;
};

// ── File-backed stores (.data/, git-ignored) ──
const dataDir = path.join(process.cwd(), '.data');
const usersFile = path.join(dataDir, 'users.json');
const pendingFile = path.join(dataDir, 'pending-codes.json');

function readJson<T>(file: string, fallback: T): T {
  try {
    return fs.existsSync(file) ? (JSON.parse(fs.readFileSync(file, 'utf8')) as T) : fallback;
  } catch (err) {
    console.error(`[authStore] could not read ${path.basename(file)}:`, err);
    return fallback;
  }
}

/**
 * Atomic write: write a temp file, then rename over the target. A crash mid-write can no
 * longer leave a half-written users.json, and the file is readable by this OS user only.
 * Node runs these synchronously on one thread, so two sign-ups can't interleave a write.
 */
function writeJsonAtomic(file: string, data: unknown) {
  try {
    fs.mkdirSync(dataDir, { recursive: true, mode: 0o700 });
    const tmp = `${file}.${process.pid}.${Date.now()}.tmp`;
    fs.writeFileSync(tmp, JSON.stringify(data, null, 2), { encoding: 'utf8', mode: 0o600 });
    fs.renameSync(tmp, file);
  } catch (err) {
    console.error(`[authStore] could not write ${path.basename(file)}:`, err);
  }
}

function loadUsers(): Map<string, UserRecord> {
  const map = new Map<string, UserRecord>();
  for (const u of readJson<UserRecord[]>(usersFile, [])) {
    if (u && typeof u.email === 'string') map.set(u.email.toLowerCase(), u);
  }
  return map;
}

function loadPending(): Map<string, PendingOtp> {
  const map = new Map<string, PendingOtp>();
  const now = Date.now();
  for (const [k, v] of Object.entries(readJson<Record<string, PendingOtp>>(pendingFile, {}))) {
    if (v && typeof v.codeHash === 'string' && v.expiresAt > now) map.set(k, v);
  }
  return map;
}

const users = globalForAuth.__spectra_users || loadUsers();
globalForAuth.__spectra_users = users;

const otps = globalForAuth.__spectra_otps || loadPending();
globalForAuth.__spectra_otps = otps;

const saveUsers = () => writeJsonAtomic(usersFile, Array.from(users.values()));
const savePending = () => writeJsonAtomic(pendingFile, Object.fromEntries(otps));

// ── Password Hashing ──
export function hashPassword(password: string): string {
  const salt = crypto.randomBytes(16).toString('hex');
  const hash = crypto.pbkdf2Sync(password, salt, 100000, 64, 'sha512').toString('hex');
  return `${salt}:${hash}`;
}

export function verifyPassword(password: string, stored: string): boolean {
  const [salt, originalHash] = stored.split(':');
  if (!salt || !originalHash) return false;
  const hash = crypto.pbkdf2Sync(password, salt, 100000, 64, 'sha512');
  const expected = Buffer.from(originalHash, 'hex');
  return expected.length === hash.length && crypto.timingSafeEqual(hash, expected);
}

// ── OTP Helpers ──
export const OTP_TTL_MS = 5 * 60 * 1000;
export const OTP_MAX_ATTEMPTS = 5;

export function generateOtp(): string {
  return crypto.randomInt(100000, 1000000).toString();
}

const hashCode = (code: string) => crypto.createHash('sha256').update(code.trim()).digest('hex');

/** Codes for sign-up and password reset live in separate namespaces so they can't be swapped. */
type CodeKind = 'signup' | 'reset' | 'login';
const otpKey = (kind: CodeKind, email: string) => `${kind}:${email.trim().toLowerCase()}`;

function setCode(kind: CodeKind, email: string, code: string, passwordHash = '', name = '', twoFactor?: boolean) {
  otps.set(otpKey(kind, email), { codeHash: hashCode(code), passwordHash, name, twoFactor, expiresAt: Date.now() + OTP_TTL_MS, attempts: 0 });
  savePending();
}

function getCode(kind: CodeKind, email: string): PendingOtp | undefined {
  const key = otpKey(kind, email);
  const entry = otps.get(key);
  if (!entry) return undefined;
  if (Date.now() > entry.expiresAt) {
    otps.delete(key);
    savePending();
    return undefined;
  }
  return entry;
}

function clearCode(kind: CodeKind, email: string) {
  if (otps.delete(otpKey(kind, email))) savePending();
}

export type CodeCheck = 'ok' | 'missing' | 'locked' | 'wrong';

/** Constant-time check that also counts and caps wrong attempts. Clears the code on success. */
function checkCode(kind: CodeKind, email: string, code: string): { result: CodeCheck; entry?: PendingOtp } {
  const entry = getCode(kind, email);
  if (!entry) return { result: 'missing' };
  if (entry.attempts >= OTP_MAX_ATTEMPTS) {
    clearCode(kind, email);
    return { result: 'locked' };
  }
  const a = Buffer.from(hashCode(String(code)), 'hex');
  const b = Buffer.from(entry.codeHash, 'hex');
  if (a.length !== b.length || !crypto.timingSafeEqual(a, b)) {
    entry.attempts += 1;
    savePending();
    return { result: 'wrong' };
  }
  clearCode(kind, email);
  return { result: 'ok', entry };
}

// Sign-up codes (carry the hashed password + name until the code is confirmed)
export const setPendingOtp = (email: string, code: string, passwordHash: string, name = '', twoFactor = true) =>
  setCode('signup', email, code, passwordHash, name, twoFactor);
export const getPendingOtp = (email: string) => getCode('signup', email);
export const checkSignupCode = (email: string, code: string) => checkCode('signup', email, code);

// Password-reset codes
export const setResetCode = (email: string, code: string) => setCode('reset', email, code);
export const checkResetCode = (email: string, code: string) => checkCode('reset', email, code);

// Login 2FA codes (only ever issued after the password was verified)
export const setLoginCode = (email: string, code: string) => setCode('login', email, code);
export const checkLoginCode = (email: string, code: string) => checkCode('login', email, code);

// ── User Management ──
export function findUser(email: string) {
  return users.get(email.trim().toLowerCase());
}

export function createUser(email: string, passwordHash: string, name = '', twoFactor = true): UserRecord {
  const key = email.trim().toLowerCase();
  const role: 'admin' | 'user' = key === ADMIN_EMAIL ? 'admin' : 'user';
  const record: UserRecord = { email: key, name, passwordHash, role, createdAt: Date.now(), twoFactor: role === 'admin' || twoFactor };
  users.set(key, record);
  saveUsers();
  return record;
}

/** Replace a password and end every session issued before now. */
export function updatePassword(email: string, passwordHash: string): UserRecord | undefined {
  const u = users.get(email.trim().toLowerCase());
  if (!u) return undefined;
  u.passwordHash = passwordHash;
  u.passwordChangedAt = Date.now();
  saveUsers();
  return u;
}

/** Change the display name (already validated with cleanName). */
export function updateName(email: string, name: string): UserRecord | undefined {
  const u = users.get(email.trim().toLowerCase());
  if (!u) return undefined;
  u.name = name;
  saveUsers();
  return u;
}

/** End every session issued before now (the caller re-issues one for the current browser). */
export function revokeSessions(email: string): UserRecord | undefined {
  const u = users.get(email.trim().toLowerCase());
  if (!u) return undefined;
  u.sessionsRevokedAt = Date.now();
  saveUsers();
  return u;
}

/** Permanently remove an account. Its sessions stop working immediately (no record = no session). */
export function deleteUser(email: string): boolean {
  const key = email.trim().toLowerCase();
  const existed = users.delete(key);
  if (existed) {
    saveUsers();
    (['signup', 'reset', 'login'] as const).forEach((k) => clearCode(k, key));
  }
  return existed;
}

/** Admins can't turn 2FA off: their account can see and manage every user. */
export const twoFactorEnforced = (u: UserRecord) => u.role === 'admin';
export const twoFactorOn = (u: UserRecord) => twoFactorEnforced(u) || !!u.twoFactor;

export function setTwoFactor(email: string, enabled: boolean): UserRecord | undefined {
  const u = users.get(email.trim().toLowerCase());
  if (!u) return undefined;
  u.twoFactor = twoFactorEnforced(u) ? true : enabled;
  saveUsers();
  return u;
}

/** Public view of every account (no password hashes), newest first. Admin-only callers. */
export function listUsers(): { email: string; name: string; role: 'admin' | 'user'; createdAt: number; twoFactor: boolean }[] {
  return Array.from(users.values())
    .map((u) => ({ email: u.email, name: u.name || '', role: u.role, createdAt: u.createdAt, twoFactor: twoFactorOn(u) }))
    .sort((a, b) => b.createdAt - a.createdAt);
}

/** Session payload for a stored user. */
export function publicUser(u: UserRecord): { email: string; role: 'admin' | 'user'; name: string } {
  return { email: u.email, role: u.role, name: u.name || '' };
}

// ── Signed Session Token (HMAC-SHA256) ──
export function createSessionToken(user: { email: string; role: 'admin' | 'user'; name?: string }): string {
  const payload = {
    email: user.email,
    role: user.role,
    name: user.name || '',
    iat: Math.floor(Date.now() / 1000),
    exp: Math.floor(Date.now() / 1000) + 7 * 24 * 60 * 60, // 7 days
  };
  const b64Payload = Buffer.from(JSON.stringify(payload)).toString('base64url');
  const signature = crypto.createHmac('sha256', authSecret()).update(b64Payload).digest('base64url');
  return `${b64Payload}.${signature}`;
}

export function verifySessionToken(token: string): { email: string; role: 'admin' | 'user'; name: string } | null {
  try {
    const [b64Payload, signature] = token.split('.');
    if (!b64Payload || !signature) return null;

    const expectedSig = crypto.createHmac('sha256', authSecret()).update(b64Payload).digest('base64url');
    if (!crypto.timingSafeEqual(Buffer.from(signature), Buffer.from(expectedSig))) {
      return null;
    }

    const payload = JSON.parse(Buffer.from(b64Payload, 'base64url').toString('utf8'));
    if (payload.exp && Date.now() / 1000 > payload.exp) {
      return null;
    }

    // Revocation: the account must still exist, and the token must post-date the last reset.
    const record = users.get(String(payload.email).toLowerCase());
    if (!record) return null;
    const revokedAt = Math.max(record.passwordChangedAt ?? 0, record.sessionsRevokedAt ?? 0);
    if (revokedAt && (payload.iat ?? 0) < Math.floor(revokedAt / 1000)) return null;

    // Role and name come from the live record, so a role change applies on the next request.
    return { email: record.email, role: record.role, name: record.name || '' };
  } catch {
    return null;
  }
}
