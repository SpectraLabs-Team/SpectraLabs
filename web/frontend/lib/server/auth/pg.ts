import crypto from 'crypto';
import { pool } from '../db';
import * as SQL from './sql';
import type { AuthBackend, CodeKind, NewUser, PendingCode, UserRecord } from './types';
import { OTP_MAX_ATTEMPTS } from './types';

type UserRow = {
  id: string;
  email: string;
  name: string;
  password_hash: string;
  role: 'admin' | 'user';
  two_factor: boolean;
  created_at_ms: number;
  password_changed_at_ms: number | null;
  workspace_id: string | null;
};

type CodeRow = {
  code_hash: string;
  password_hash: string;
  name: string;
  two_factor: boolean | null;
  attempts: number;
  expires_at_ms: number;
  expired: boolean;
};

const toUser = (r: UserRow): UserRecord => ({
  id: r.id,
  email: r.email,
  name: r.name,
  passwordHash: r.password_hash,
  role: r.role,
  twoFactor: r.two_factor,
  createdAt: Math.round(Number(r.created_at_ms)),
  passwordChangedAt: r.password_changed_at_ms == null ? undefined : Math.round(Number(r.password_changed_at_ms)),
  workspaceId: r.workspace_id ?? undefined,
});

const toCode = (r: CodeRow): PendingCode => ({
  codeHash: r.code_hash,
  passwordHash: r.password_hash,
  name: r.name,
  twoFactor: r.two_factor ?? undefined,
  attempts: r.attempts,
  expiresAt: Math.round(Number(r.expires_at_ms)),
});

const sameHash = (a: string, b: string) => {
  const x = Buffer.from(a, 'hex');
  const y = Buffer.from(b, 'hex');
  return x.length === y.length && x.length > 0 && crypto.timingSafeEqual(x, y);
};

/** Accounts and codes in Postgres (tables from crates/spectra-db/migrations). */
export function pgBackend(): AuthBackend {
  const findUser = async (email: string) => {
    const { rows } = await pool().query<UserRow>(SQL.FIND_USER, [email]);
    return rows[0] ? toUser(rows[0]) : undefined;
  };

  return {
    findUser,

    async insertUser(u: NewUser) {
      await pool().query(SQL.INSERT_USER, [u.email, u.name || '', u.passwordHash, u.role, !!u.twoFactor]);
      const created = await findUser(u.email);
      if (!created) throw new Error('user insert failed');
      return created;
    },

    async updateUser(email, patch) {
      const { rowCount } = await pool().query(SQL.UPDATE_USER, [
        email,
        patch.passwordHash ?? null,
        patch.passwordChangedAt ?? null,
        patch.twoFactor ?? null,
      ]);
      return rowCount ? findUser(email) : undefined;
    },

    async listUsers() {
      const { rows } = await pool().query<UserRow>(SQL.LIST_USERS);
      return rows.map(toUser);
    },

    async putCode(kind: CodeKind, email: string, c: PendingCode) {
      await pool().query(SQL.PUT_CODE, [kind, email, c.codeHash, c.passwordHash, c.name, c.twoFactor ?? null, c.expiresAt]);
    },

    async peekCode(kind: CodeKind, email: string) {
      const { rows } = await pool().query<CodeRow>(SQL.PEEK_CODE, [kind, email]);
      const row = rows[0];
      if (!row) return undefined;
      if (row.expired) {
        await pool().query(SQL.DELETE_CODE, [kind, email]);
        return undefined;
      }
      return toCode(row);
    },

    async consumeCode(kind: CodeKind, email: string, codeHash: string) {
      const client = await pool().connect();
      try {
        await client.query('BEGIN');
        const { rows } = await client.query<CodeRow>(SQL.LOCK_CODE, [kind, email]);
        const row = rows[0];
        let outcome: { result: 'ok' | 'missing' | 'locked' | 'wrong'; entry?: PendingCode };
        if (!row || row.expired) {
          if (row) await client.query(SQL.DELETE_CODE, [kind, email]);
          outcome = { result: 'missing' };
        } else if (row.attempts >= OTP_MAX_ATTEMPTS) {
          await client.query(SQL.DELETE_CODE, [kind, email]);
          outcome = { result: 'locked' };
        } else if (!sameHash(row.code_hash, codeHash)) {
          await client.query(SQL.BUMP_ATTEMPTS, [kind, email]);
          outcome = { result: 'wrong' };
        } else {
          await client.query(SQL.DELETE_CODE, [kind, email]);
          outcome = { result: 'ok', entry: toCode(row) };
        }
        await client.query('COMMIT');
        return outcome;
      } catch (err) {
        await client.query('ROLLBACK').catch(() => undefined);
        throw err;
      } finally {
        client.release();
      }
    },
  };
}
