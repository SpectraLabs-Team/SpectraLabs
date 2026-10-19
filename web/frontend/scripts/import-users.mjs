// One-off: copy accounts from the old file store (.data/users.json) into Postgres.
//   npm run db:import-users                 import (applies migrations first)
//   npm run db:import-users -- --dry-run    show what would happen, change nothing
//   npm run db:import-users -- path/to/users.json
//
// Safe to run more than once: an email that already exists in the database is skipped, never
// overwritten. Password hashes, roles and 2FA settings are kept, so everyone can still sign in
// with their current password. (Old signed-cookie sessions don't carry over: everyone signs in once.)
// Pending codes (.data/pending-codes.json) are not imported: they expire after 5 minutes anyway.
//
// Once the import is verified (sign in with an imported account), delete web/frontend/.data/.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import pg from 'pg';
import { loadEnv, root } from './load-env.mjs';
import { migrate } from './db-migrate.mjs';

const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

/** Validate and normalise one record from users.json. Returns [row, null] or [null, reason]. */
export function toRow(u) {
  if (!u || typeof u !== 'object') return [null, 'not an object'];
  const email = typeof u.email === 'string' ? u.email.trim().toLowerCase() : '';
  if (!EMAIL_RE.test(email)) return [null, `invalid email ${JSON.stringify(u.email)}`];
  const isArgon2 = typeof u.passwordHash === 'string' && u.passwordHash.startsWith('$argon2id$');
  const isLegacy = typeof u.passwordHash === 'string' && /^[0-9a-f]+:[0-9a-f]+$/i.test(u.passwordHash);
  if (!isArgon2 && !isLegacy) return [null, `${email}: missing or malformed password hash`];
  const role = u.role === 'admin' ? 'admin' : 'user';
  const ts = (ms) => (Number.isFinite(ms) && ms > 0 ? new Date(ms) : null);
  return [
    {
      email,
      name: typeof u.name === 'string' ? u.name : '',
      password_hash: u.passwordHash,
      platform_role: role,
      // Old records without the field predate the per-account setting; keep them on (the safe side).
      two_factor: role === 'admin' || u.twoFactor !== false,
      created_at: ts(u.createdAt) ?? new Date(),
      password_changed_at: ts(u.passwordChangedAt),
    },
    null,
  ];
}

/** Insert rows with an open pg client; existing emails are skipped. Returns { imported, skipped }. */
export async function importRows(client, rows) {
  const imported = [];
  const skipped = [];
  await client.query('BEGIN');
  try {
    for (const r of rows) {
      const res = await client.query(
        `INSERT INTO users (email, name, password_hash, platform_role, two_factor, created_at, password_changed_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (email) DO NOTHING
         RETURNING email`,
        [r.email, r.name, r.password_hash, r.platform_role, r.two_factor, r.created_at, r.password_changed_at],
      );
      (res.rowCount ? imported : skipped).push(r.email);
    }
    // Every account gets its personal workspace (owner), as createUser does for new sign-ups.
    await client.query(`
      WITH created AS (
        INSERT INTO workspaces (name, slug, created_by, personal_owner_id)
        SELECT 'Personal',
               coalesce(nullif(btrim(left(regexp_replace(lower(split_part(u.email, '@', 1)), '[^a-z0-9]+', '-', 'g'), 40), '-'), ''), 'user')
                 || '-' || substr(replace(gen_random_uuid()::text, '-', ''), 1, 6),
               u.id, u.id
          FROM users u
         WHERE NOT EXISTS (SELECT 1 FROM workspaces w WHERE w.personal_owner_id = u.id)
        RETURNING id, personal_owner_id
      )
      INSERT INTO workspace_members (workspace_id, user_id, role) SELECT id, personal_owner_id, 'owner' FROM created`);
    await client.query('COMMIT');
  } catch (err) {
    await client.query('ROLLBACK').catch(() => {});
    throw err;
  }
  return { imported, skipped };
}

async function main() {
  loadEnv();
  const args = process.argv.slice(2);
  const dryRun = args.includes('--dry-run');
  const file = path.resolve(args.find((a) => !a.startsWith('--')) || path.join(root, '.data', 'users.json'));

  if (!fs.existsSync(file)) {
    console.log(`Nothing to import: ${path.relative(process.cwd(), file) || file} does not exist.`);
    return;
  }
  let records;
  try {
    records = JSON.parse(fs.readFileSync(file, 'utf8'));
    if (!Array.isArray(records)) throw new Error('expected a JSON array');
  } catch (err) {
    console.error(`✖ Could not read ${file}: ${err.message}`);
    process.exit(1);
  }

  const rows = [];
  const seen = new Set();
  for (const u of records) {
    const [row, problem] = toRow(u);
    if (problem) console.warn(`  skip: ${problem}`);
    else if (seen.has(row.email)) console.warn(`  skip: ${row.email} appears twice in the file`);
    else {
      seen.add(row.email);
      rows.push(row);
    }
  }

  if (!process.env.DATABASE_URL) {
    console.error('✖ DATABASE_URL is not set. Run `npm run setup` first.');
    process.exit(1);
  }
  const client = new pg.Client({ connectionString: process.env.DATABASE_URL });
  await client.connect();
  try {
    if (dryRun) {
      const existing = new Set(
        (await client.query('SELECT email FROM users WHERE email = ANY($1)', [rows.map((r) => r.email)]).catch(() => ({ rows: [] }))).rows.map((r) => r.email),
      );
      for (const r of rows) console.log(`  ${existing.has(r.email) ? 'exists, would skip' : 'would import'}: ${r.email} (${r.platform_role})`);
      console.log(`Dry run: ${rows.filter((r) => !existing.has(r.email)).length} to import, ${existing.size} already in the database. Nothing changed.`);
      return;
    }
    await migrate(client, { log: (s) => console.log(`[db] ${s}`) });
    const { imported, skipped } = await importRows(client, rows);
    for (const e of imported) console.log(`  imported: ${e}`);
    for (const e of skipped) console.log(`  already in the database, skipped: ${e}`);
    console.log(`Done: ${imported.length} imported, ${skipped.length} skipped.`);
    if (imported.length || skipped.length) {
      console.log('Next: sign in with one of these accounts to confirm, then delete web/frontend/.data/.');
    }
  } finally {
    await client.end();
  }
}

const invokedDirectly = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) {
  main().catch((err) => {
    console.error('✖ Import failed:', err.message);
    process.exit(1);
  });
}
