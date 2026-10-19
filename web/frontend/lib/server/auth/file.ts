import crypto from 'crypto';
import fs from 'fs';
import path from 'path';
import type { AuthBackend, CodeKind, NewUser, PendingCode, UserRecord } from './types';
import { OTP_MAX_ATTEMPTS } from './types';

/**
 * Local-development store: .data/users.json and .data/pending-codes.json (git-ignored). Used only
 * when DATABASE_URL is unset, and refused in production. Node runs these synchronously on one
 * thread, so two sign-ups can't interleave a write; a crash mid-write can't leave a half-written
 * file (write a temp file, then rename).
 */

const g = globalThis as unknown as {
  __spectra_users?: Map<string, UserRecord>;
  __spectra_codes?: Map<string, PendingCode>;
};

const dataDir = () => path.join(process.cwd(), '.data');
const usersFile = () => path.join(dataDir(), 'users.json');
const codesFile = () => path.join(dataDir(), 'pending-codes.json');

function readJson<T>(file: string, fallback: T): T {
  try {
    return fs.existsSync(file) ? (JSON.parse(fs.readFileSync(file, 'utf8')) as T) : fallback;
  } catch (err) {
    console.error(`[authStore] could not read ${path.basename(file)}:`, err);
    return fallback;
  }
}

function writeJsonAtomic(file: string, data: unknown) {
  try {
    fs.mkdirSync(dataDir(), { recursive: true, mode: 0o700 });
    const tmp = `${file}.${process.pid}.${Date.now()}.tmp`;
    fs.writeFileSync(tmp, JSON.stringify(data, null, 2), { encoding: 'utf8', mode: 0o600 });
    fs.renameSync(tmp, file);
  } catch (err) {
    console.error(`[authStore] could not write ${path.basename(file)}:`, err);
  }
}

function users(): Map<string, UserRecord> {
  if (!g.__spectra_users) {
    const map = new Map<string, UserRecord>();
    let changed = false;
    for (const u of readJson<UserRecord[]>(usersFile(), [])) {
      if (!u || typeof u.email !== 'string') continue;
      if (!u.id) {
        u.id = crypto.randomUUID(); // records written before ids existed
        changed = true;
      }
      map.set(u.email.toLowerCase(), u);
    }
    g.__spectra_users = map;
    if (changed) saveUsers();
  }
  return g.__spectra_users;
}

function codes(): Map<string, PendingCode> {
  if (!g.__spectra_codes) {
    const map = new Map<string, PendingCode>();
    const now = Date.now();
    for (const [k, v] of Object.entries(readJson<Record<string, PendingCode>>(codesFile(), {}))) {
      if (v && typeof v.codeHash === 'string' && v.expiresAt > now) map.set(k, v);
    }
    g.__spectra_codes = map;
  }
  return g.__spectra_codes;
}

const saveUsers = () => writeJsonAtomic(usersFile(), Array.from(users().values()));
const saveCodes = () => writeJsonAtomic(codesFile(), Object.fromEntries(codes()));
const key = (kind: CodeKind, email: string) => `${kind}:${email.trim().toLowerCase()}`;
const norm = (email: string) => email.trim().toLowerCase();

function liveCode(kind: CodeKind, email: string): PendingCode | undefined {
  const k = key(kind, email);
  const entry = codes().get(k);
  if (!entry) return undefined;
  if (Date.now() > entry.expiresAt) {
    codes().delete(k);
    saveCodes();
    return undefined;
  }
  return entry;
}

export function fileBackend(): AuthBackend {
  return {
    async findUser(email) {
      return users().get(norm(email));
    },

    async insertUser(u: NewUser) {
      const existing = users().get(norm(u.email));
      if (existing) return existing;
      const record: UserRecord = { ...u, id: crypto.randomUUID(), email: norm(u.email), createdAt: Date.now() };
      users().set(record.email, record);
      saveUsers();
      return record;
    },

    async updateUser(email, patch) {
      const u = users().get(norm(email));
      if (!u) return undefined;
      if (patch.passwordHash !== undefined) u.passwordHash = patch.passwordHash;
      if (patch.passwordChangedAt !== undefined) u.passwordChangedAt = patch.passwordChangedAt;
      if (patch.twoFactor !== undefined) u.twoFactor = patch.twoFactor;
      saveUsers();
      return u;
    },

    async listUsers() {
      return Array.from(users().values()).sort((a, b) => b.createdAt - a.createdAt);
    },

    async putCode(kind, email, c) {
      codes().set(key(kind, email), { ...c, attempts: 0 });
      saveCodes();
    },

    async peekCode(kind, email) {
      return liveCode(kind, email);
    },

    async consumeCode(kind, email, codeHash) {
      const entry = liveCode(kind, email);
      if (!entry) return { result: 'missing' };
      if (entry.attempts >= OTP_MAX_ATTEMPTS) {
        codes().delete(key(kind, email));
        saveCodes();
        return { result: 'locked' };
      }
      const a = Buffer.from(codeHash, 'hex');
      const b = Buffer.from(entry.codeHash, 'hex');
      if (a.length !== b.length || !crypto.timingSafeEqual(a, b)) {
        entry.attempts += 1;
        saveCodes();
        return { result: 'wrong' };
      }
      codes().delete(key(kind, email));
      saveCodes();
      return { result: 'ok', entry };
    },
  };
}
