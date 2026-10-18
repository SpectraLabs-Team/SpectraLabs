import crypto from 'crypto';
import fs from 'fs';
import path from 'path';

export const ADMIN_EMAIL = (process.env.ADMIN_EMAIL || 'spectralabs.team@gmail.com').toLowerCase();
const rawSecret = process.env.AUTH_SECRET;
if (!rawSecret && process.env.NODE_ENV === 'production') {
  throw new Error('FATAL: AUTH_SECRET environment variable is missing in production.');
}
const SECRET = rawSecret || 'spectralabs_jwt_default_secret_key_32bytes_required';

export type UserRecord = { email: string; name?: string; passwordHash: string; role: 'admin' | 'user'; createdAt: number };

// ── In-Memory OTP Store (5-minute TTL) ──
type PendingOtp = {
  code: string;
  passwordHash: string;
  name: string;
  expiresAt: number;
  attempts: number;
};

// Global map survives hot module reloading in development
const globalForAuth = global as unknown as {
  __spectra_otps?: Map<string, PendingOtp>;
  __spectra_users?: Map<string, UserRecord>;
};

const otps = globalForAuth.__spectra_otps || new Map<string, PendingOtp>();
globalForAuth.__spectra_otps = otps;

// ── Persistent User Store ──
const dataDir = path.join(process.cwd(), '.data');
const usersFile = path.join(dataDir, 'users.json');

function loadUsers(): Map<string, UserRecord> {
  const map = new Map<string, UserRecord>();
  try {
    if (fs.existsSync(usersFile)) {
      const data = JSON.parse(fs.readFileSync(usersFile, 'utf8'));
      for (const u of data) {
        map.set(u.email.toLowerCase(), u);
      }
    }
  } catch (err) {
    console.error('Failed to load users store:', err);
  }
  return map;
}

function saveUsers(map: Map<string, UserRecord>) {
  try {
    if (!fs.existsSync(dataDir)) {
      fs.mkdirSync(dataDir, { recursive: true });
    }
    const arr = Array.from(map.values());
    fs.writeFileSync(usersFile, JSON.stringify(arr, null, 2), 'utf8');
  } catch (err) {
    console.error('Failed to save users store:', err);
  }
}

const users = globalForAuth.__spectra_users || loadUsers();
globalForAuth.__spectra_users = users;

// ── Password Hashing ──
export function hashPassword(password: string): string {
  const salt = crypto.randomBytes(16).toString('hex');
  const hash = crypto.pbkdf2Sync(password, salt, 100000, 64, 'sha512').toString('hex');
  return `${salt}:${hash}`;
}

export function verifyPassword(password: string, stored: string): boolean {
  const [salt, originalHash] = stored.split(':');
  if (!salt || !originalHash) return false;
  const hash = crypto.pbkdf2Sync(password, salt, 100000, 64, 'sha512').toString('hex');
  return crypto.timingSafeEqual(Buffer.from(hash, 'hex'), Buffer.from(originalHash, 'hex'));
}

// ── OTP Helpers ──
export function generateOtp(): string {
  return crypto.randomInt(100000, 999999).toString();
}

export function setPendingOtp(email: string, code: string, passwordHash: string, name = '') {
  const key = email.trim().toLowerCase();
  otps.set(key, {
    code,
    passwordHash,
    name,
    expiresAt: Date.now() + 5 * 60 * 1000, // 5 minutes
    attempts: 0,
  });
}

export function getPendingOtp(email: string): PendingOtp | undefined {
  const key = email.trim().toLowerCase();
  const entry = otps.get(key);
  if (!entry) return undefined;
  if (Date.now() > entry.expiresAt) {
    otps.delete(key);
    return undefined;
  }
  return entry;
}

export function clearPendingOtp(email: string) {
  otps.delete(email.trim().toLowerCase());
}

// ── User Management ──
export function findUser(email: string) {
  return users.get(email.trim().toLowerCase());
}

export function createUser(email: string, passwordHash: string, name = ''): UserRecord {
  const key = email.trim().toLowerCase();
  const role: 'admin' | 'user' = key === ADMIN_EMAIL ? 'admin' : 'user';
  const record: UserRecord = { email: key, name, passwordHash, role, createdAt: Date.now() };
  users.set(key, record);
  saveUsers(users);
  return record;
}

/** Public view of every account (no password hashes), newest first. Admin-only callers. */
export function listUsers(): { email: string; name: string; role: 'admin' | 'user'; createdAt: number }[] {
  return Array.from(users.values())
    .map((u) => ({ email: u.email, name: u.name || '', role: u.role, createdAt: u.createdAt }))
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
  const signature = crypto.createHmac('sha256', SECRET).update(b64Payload).digest('base64url');
  return `${b64Payload}.${signature}`;
}

export function verifySessionToken(token: string): { email: string; role: 'admin' | 'user'; name: string } | null {
  try {
    const [b64Payload, signature] = token.split('.');
    if (!b64Payload || !signature) return null;

    const expectedSig = crypto.createHmac('sha256', SECRET).update(b64Payload).digest('base64url');
    if (!crypto.timingSafeEqual(Buffer.from(signature), Buffer.from(expectedSig))) {
      return null;
    }

    const payload = JSON.parse(Buffer.from(b64Payload, 'base64url').toString('utf8'));
    if (payload.exp && Date.now() / 1000 > payload.exp) {
      return null;
    }

    return { email: payload.email, role: payload.role, name: typeof payload.name === 'string' ? payload.name : '' };
  } catch {
    return null;
  }
}
