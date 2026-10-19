import crypto from 'crypto';
import type { PoolClient } from 'pg';
import { record as audit } from './audit';
import { query, transaction } from './db';

/**
 * Encrypted secret storage for broker API keys (M19 task 8). Server-only.
 *
 * Envelope encryption with AES-256-GCM:
 *   master key (SECRETS_MASTER_KEY, outside the database)
 *     └─ wraps one random data key per workspace      (table workspace_keys)
 *          └─ encrypts that workspace's secrets       (table secrets)
 * Every ciphertext is bound to where it belongs (associated data), so a row copied into another
 * workspace or kind fails to decrypt instead of leaking.
 *
 * Write-only from the UI: putSecret / deleteSecret / listSecrets never return a secret, only its
 * kind, the last 4 characters of the key id, and timestamps. useSecret decrypts, and it is for the
 * worker that places broker calls, at the moment of the call: never import it from a route
 * handler or component (a test enforces this), and never log or return what it hands you.
 *
 * SECRETS_MASTER_KEY: base64 of 32 random bytes (`npm run setup` makes one), or a keyring
 * "2:<base64>,1:<base64>" during rotation: the first entry wraps new keys, the others can still
 * unwrap. Rotate with `npm run secrets:rotate`. Lose the master key and every stored secret is
 * unrecoverable: back it up somewhere other than the database backups.
 */

// ── Kinds of secret ──

type Field = { label: string; pattern: RegExp; hint?: boolean };
export const SECRET_KINDS = {
  alpaca_paper: {
    label: 'Alpaca paper trading',
    fields: {
      keyId: { label: 'API key ID', pattern: /^[A-Za-z0-9]{8,64}$/, hint: true },
      secretKey: { label: 'Secret key', pattern: /^[A-Za-z0-9/+=_-]{16,128}$/ },
    } as Record<string, Field>,
  },
} as const;
export type SecretKind = keyof typeof SECRET_KINDS;
export const isSecretKind = (k: unknown): k is SecretKind => typeof k === 'string' && Object.prototype.hasOwnProperty.call(SECRET_KINDS, k);

export class SecretValidationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'SecretValidationError';
  }
}

export class VaultConfigError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'VaultConfigError';
  }
}

/** Check and normalise a submitted secret. Error messages name the field, never echo the value. */
export function validateSecret(kind: SecretKind, input: unknown): Record<string, string> {
  if (!input || typeof input !== 'object') throw new SecretValidationError('Missing key fields.');
  const out: Record<string, string> = {};
  for (const [name, f] of Object.entries(SECRET_KINDS[kind].fields)) {
    const v = (input as Record<string, unknown>)[name];
    const clean = typeof v === 'string' ? v.trim() : '';
    if (!f.pattern.test(clean)) throw new SecretValidationError(`${f.label} doesn't look right. Paste it exactly as Alpaca shows it.`);
    out[name] = clean;
  }
  return out;
}

const hintOf = (kind: SecretKind, value: Record<string, string>) => {
  const field = Object.entries(SECRET_KINDS[kind].fields).find(([, f]) => f.hint)?.[0];
  return field ? value[field].slice(-4) : '';
};

// ── Master keyring ──

type Keyring = { current: { version: number; key: Buffer }; byVersion: Map<number, Buffer> };

/** Parse SECRETS_MASTER_KEY ("<base64>" = version 1, or "v:<base64>,v:<base64>", first = current). */
export function parseKeyring(raw: string | undefined): Keyring {
  if (!raw || !raw.trim()) throw new VaultConfigError('SECRETS_MASTER_KEY is not set. Run `npm run setup` to generate one.');
  const entries = raw.split(',').map((e) => e.trim()).filter(Boolean);
  const byVersion = new Map<number, Buffer>();
  let current: Keyring['current'] | null = null;
  for (const entry of entries) {
    const m = entry.match(/^(?:(\d{1,6}):)?([A-Za-z0-9+/_=-]+)$/);
    if (!m || (entries.length > 1 && !m[1])) throw new VaultConfigError('SECRETS_MASTER_KEY must be base64, or "version:base64" entries separated by commas.');
    const version = m[1] ? Number(m[1]) : 1;
    const key = Buffer.from(m[2], m[2].includes('-') || m[2].includes('_') ? 'base64url' : 'base64');
    if (key.length !== 32) throw new VaultConfigError(`SECRETS_MASTER_KEY version ${version} must be exactly 32 bytes (base64 of 32 random bytes).`);
    if (byVersion.has(version)) throw new VaultConfigError(`SECRETS_MASTER_KEY lists version ${version} twice.`);
    byVersion.set(version, key);
    current ??= { version, key };
  }
  if (!current) throw new VaultConfigError('SECRETS_MASTER_KEY is empty.');
  return { current, byVersion };
}

const keyring = () => parseKeyring(process.env.SECRETS_MASTER_KEY);

// ── AES-256-GCM ──

const FORMAT = 1;

export function seal(key: Buffer, plaintext: Buffer, aad: string): Buffer {
  const iv = crypto.randomBytes(12);
  const cipher = crypto.createCipheriv('aes-256-gcm', key, iv);
  cipher.setAAD(Buffer.from(aad, 'utf8'));
  const ct = Buffer.concat([cipher.update(plaintext), cipher.final()]);
  return Buffer.concat([Buffer.from([FORMAT]), iv, cipher.getAuthTag(), ct]);
}

export function open(key: Buffer, blob: Buffer, aad: string): Buffer {
  if (blob.length < 29 || blob[0] !== FORMAT) throw new Error('vault: unknown ciphertext format');
  const decipher = crypto.createDecipheriv('aes-256-gcm', key, blob.subarray(1, 13));
  decipher.setAAD(Buffer.from(aad, 'utf8'));
  decipher.setAuthTag(blob.subarray(13, 29));
  return Buffer.concat([decipher.update(blob.subarray(29)), decipher.final()]);
}

const dataKeyAad = (workspaceId: string) => `spectra:workspace-key:${workspaceId}`;
const secretAad = (workspaceId: string, kind: SecretKind) => `spectra:secret:${workspaceId}:${kind}`;

type Queryable = Pick<PoolClient, 'query'>;

/** The workspace's data key, created (and wrapped with the current master key) on first use. */
async function dataKey(c: Queryable, workspaceId: string, ring: Keyring): Promise<Buffer> {
  let [row] = await query<{ wrapped_key: Buffer; master_version: number }>('SELECT wrapped_key, master_version FROM workspace_keys WHERE workspace_id = $1', [workspaceId], c);
  if (!row) {
    const fresh = crypto.randomBytes(32);
    await query(
      'INSERT INTO workspace_keys (workspace_id, wrapped_key, master_version) VALUES ($1, $2, $3) ON CONFLICT (workspace_id) DO NOTHING',
      [workspaceId, seal(ring.current.key, fresh, dataKeyAad(workspaceId)), ring.current.version],
      c,
    );
    fresh.fill(0);
    [row] = await query<{ wrapped_key: Buffer; master_version: number }>('SELECT wrapped_key, master_version FROM workspace_keys WHERE workspace_id = $1', [workspaceId], c);
  }
  const master = ring.byVersion.get(row.master_version);
  if (!master) throw new VaultConfigError(`This workspace's key is wrapped with master key version ${row.master_version}, which is not in SECRETS_MASTER_KEY.`);
  return open(master, row.wrapped_key, dataKeyAad(workspaceId));
}

// ── Write-only API (for route handlers) ──

export type SecretInfo = {
  kind: SecretKind;
  label: string;
  /** Last 4 characters of the key id. */
  hint: string;
  createdAt: number;
  updatedAt: number;
  lastUsedAt: number | null;
  createdBy: string | null;
};

type InfoRow = { kind: SecretKind; hint: string; created_at: Date; updated_at: Date; last_used_at: Date | null; created_by_email: string | null };

const toInfo = (r: InfoRow): SecretInfo => ({
  kind: r.kind,
  label: SECRET_KINDS[r.kind].label,
  hint: r.hint,
  createdAt: r.created_at.getTime(),
  updatedAt: r.updated_at.getTime(),
  lastUsedAt: r.last_used_at?.getTime() ?? null,
  createdBy: r.created_by_email,
});

const INFO_SELECT = `
  SELECT s.kind, s.hint, s.created_at, s.updated_at, s.last_used_at, u.email AS created_by_email
    FROM secrets s LEFT JOIN users u ON u.id = s.created_by`;

/** Add or replace a workspace's secret of this kind. Returns metadata only. */
export async function putSecret(workspaceId: string, kind: SecretKind, input: unknown, userId: string): Promise<SecretInfo> {
  const value = validateSecret(kind, input);
  const ring = keyring();
  return transaction(async (c) => {
    const key = await dataKey(c, workspaceId, ring);
    const plaintext = Buffer.from(JSON.stringify(value), 'utf8');
    const ciphertext = seal(key, plaintext, secretAad(workspaceId, kind));
    plaintext.fill(0);
    key.fill(0);
    await query(
      `INSERT INTO secrets (workspace_id, kind, ciphertext, hint, created_by)
       VALUES ($1, $2, $3, $4, $5)
       ON CONFLICT (workspace_id, kind) DO UPDATE SET
         ciphertext = EXCLUDED.ciphertext, hint = EXCLUDED.hint, created_by = EXCLUDED.created_by,
         updated_at = now(), last_used_at = NULL`,
      [workspaceId, kind, ciphertext, hintOf(kind, value), userId],
      c,
    );
    const [row] = await query<InfoRow>(`${INFO_SELECT} WHERE s.workspace_id = $1 AND s.kind = $2`, [workspaceId, kind], c);
    return toInfo(row);
  });
}

/** Metadata for every secret in a workspace (never the secret itself). */
export async function listSecrets(workspaceId: string): Promise<SecretInfo[]> {
  const rows = await query<InfoRow>(`${INFO_SELECT} WHERE s.workspace_id = $1 ORDER BY s.kind`, [workspaceId]);
  return rows.map(toInfo);
}

export async function deleteSecret(workspaceId: string, kind: SecretKind): Promise<boolean> {
  return (await query('DELETE FROM secrets WHERE workspace_id = $1 AND kind = $2 RETURNING id', [workspaceId, kind])).length > 0;
}

// ── Decryption (worker only) ──

/**
 * WORKER ONLY. Decrypt a workspace's secret, hand it to `fn` for the broker call, and record the
 * use. The plaintext exists only for the duration of `fn`; don't log it, store it or return it.
 * Returns whatever `fn` returns; throws if there is no such secret. Each use is audit-logged
 * (`context.actor`: who triggered it, e.g. the user who placed the order; `purpose`: e.g. "order").
 */
export async function useSecret<T>(
  workspaceId: string,
  kind: SecretKind,
  fn: (value: Readonly<Record<string, string>>) => Promise<T>,
  context: { actor?: { id?: string | null; email?: string | null } | null; purpose?: string } = {},
): Promise<T> {
  const ring = keyring();
  const value = await transaction(async (c) => {
    const [row] = await query<{ ciphertext: Buffer }>('SELECT ciphertext FROM secrets WHERE workspace_id = $1 AND kind = $2', [workspaceId, kind], c);
    if (!row) throw new Error(`vault: no ${kind} secret for this workspace`);
    const key = await dataKey(c, workspaceId, ring);
    try {
      const plaintext = open(key, row.ciphertext, secretAad(workspaceId, kind));
      const parsed = JSON.parse(plaintext.toString('utf8')) as Record<string, string>;
      plaintext.fill(0);
      return Object.freeze(parsed);
    } finally {
      key.fill(0);
    }
  });
  await query('UPDATE secrets SET last_used_at = now() WHERE workspace_id = $1 AND kind = $2', [workspaceId, kind]);
  await audit({ action: 'secret.used', actor: context.actor ?? null, workspaceId, target: { type: 'secret', id: kind }, details: context.purpose ? { purpose: context.purpose } : {} });
  return fn(value);
}

// ── Per-user secrets (authenticator seeds) ──
// Small values encrypted directly with the master key, bound to the user id. Rotation re-encrypts
// them along with the workspace data keys.

const userSecretAad = (userId: string) => `spectra:totp:${userId}`;

export function sealUserSecret(userId: string, plaintext: Buffer): { ciphertext: Buffer; version: number } {
  const ring = keyring();
  return { ciphertext: seal(ring.current.key, plaintext, userSecretAad(userId)), version: ring.current.version };
}

export function openUserSecret(userId: string, ciphertext: Buffer, version: number): Buffer {
  const master = keyring().byVersion.get(version);
  if (!master) throw new VaultConfigError(`An authenticator secret is encrypted with master key version ${version}, which is not in SECRETS_MASTER_KEY.`);
  return open(master, ciphertext, userSecretAad(userId));
}

// ── Master-key rotation ──

/**
 * Re-wrap every workspace data key (and re-encrypt every authenticator secret) that isn't under the
 * current master key version. Stored broker keys themselves are untouched (their data keys don't
 * change). Returns how many rows were re-wrapped.
 * Run with the new key first in SECRETS_MASTER_KEY and the old one still listed; once this
 * reports 0 left, the old entry can be removed.
 */
export async function rotateMasterKey(): Promise<number> {
  const ring = keyring();
  let done = 0;
  let firstError: unknown = null;
  // Each stage runs even if the other fails (e.g. one row needs an old master key that's no longer
  // listed), so as much as possible is rotated; the first error is reported at the end.
  for (const stage of [rewrapWorkspaceKeys, reencryptUserSecrets]) {
    try {
      done += await stage(ring);
    } catch (err) {
      firstError ??= err;
    }
  }
  if (firstError) throw firstError;
  return done;
}

async function rewrapWorkspaceKeys(ring: Keyring): Promise<number> {
  let done = 0;
  for (;;) {
    const batch = await transaction(async (c) => {
      const rows = await query<{ workspace_id: string; wrapped_key: Buffer; master_version: number }>(
        'SELECT workspace_id, wrapped_key, master_version FROM workspace_keys WHERE master_version <> $1 LIMIT 100 FOR UPDATE SKIP LOCKED',
        [ring.current.version],
        c,
      );
      for (const r of rows) {
        const old = ring.byVersion.get(r.master_version);
        if (!old) throw new VaultConfigError(`Master key version ${r.master_version} is needed to re-wrap workspace keys but is not in SECRETS_MASTER_KEY.`);
        const key = open(old, r.wrapped_key, dataKeyAad(r.workspace_id));
        await query('UPDATE workspace_keys SET wrapped_key = $2, master_version = $3, rotated_at = now() WHERE workspace_id = $1', [
          r.workspace_id,
          seal(ring.current.key, key, dataKeyAad(r.workspace_id)),
          ring.current.version,
        ], c);
        key.fill(0);
      }
      return rows.length;
    });
    done += batch;
    if (batch === 0) return done;
  }
}

async function reencryptUserSecrets(ring: Keyring): Promise<number> {
  let done = 0;
  for (;;) {
    const batch = await transaction(async (c) => {
      const rows = await query<{ user_id: string; secret_ciphertext: Buffer; master_version: number }>(
        'SELECT user_id, secret_ciphertext, master_version FROM user_totp WHERE master_version <> $1 LIMIT 100 FOR UPDATE SKIP LOCKED',
        [ring.current.version],
        c,
      );
      for (const r of rows) {
        const old = ring.byVersion.get(r.master_version);
        if (!old) throw new VaultConfigError(`Master key version ${r.master_version} is needed to re-encrypt authenticator secrets but is not in SECRETS_MASTER_KEY.`);
        const plain = open(old, r.secret_ciphertext, userSecretAad(r.user_id));
        await query('UPDATE user_totp SET secret_ciphertext = $2, master_version = $3 WHERE user_id = $1', [r.user_id, seal(ring.current.key, plain, userSecretAad(r.user_id)), ring.current.version], c);
        plain.fill(0);
      }
      return rows.length;
    });
    done += batch;
    if (batch === 0) return done;
  }
}
