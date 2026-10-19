// Apply the SQL files in db/migrations, in name order, once each.
//   npm run db:migrate            apply pending migrations
//   node scripts/db-migrate.mjs --create
//                                 also create the database if it doesn't exist (dev convenience;
//                                 `npm run dev` passes this)
//   node scripts/db-migrate.mjs --status
//                                 list applied and pending migrations
//
// Applied files are recorded in schema_migrations with a checksum. Never edit a migration that
// has been applied anywhere: add a new numbered file instead.
// Safe to run from several servers at once: a Postgres advisory lock serialises them.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { pathToFileURL } from 'node:url';
import pg from 'pg';
import { loadEnv, root } from './load-env.mjs';

const MIGRATIONS_DIR = path.join(root, 'db', 'migrations');
const LOCK_ID = 7_312_019; // arbitrary constant: "only one migrator at a time"

export function migrationFiles(dir = MIGRATIONS_DIR) {
  return fs
    .readdirSync(dir)
    .filter((f) => /^\d+_.+\.sql$/.test(f))
    .sort()
    .map((name) => {
      const sql = fs.readFileSync(path.join(dir, name), 'utf8');
      return { name, sql, checksum: crypto.createHash('sha256').update(sql).digest('hex') };
    });
}

/**
 * Apply pending migrations using an open pg.Client. Returns the names applied.
 * Each file runs in its own transaction together with its schema_migrations row.
 */
export async function migrate(client, { log = () => {}, dir } = {}) {
  await client.query('SELECT pg_advisory_lock($1)', [LOCK_ID]);
  try {
    await client.query(`CREATE TABLE IF NOT EXISTS schema_migrations (
      name       text PRIMARY KEY,
      checksum   text NOT NULL,
      applied_at timestamptz NOT NULL DEFAULT now()
    )`);
    const done = new Map((await client.query('SELECT name, checksum FROM schema_migrations')).rows.map((r) => [r.name, r.checksum]));
    const applied = [];
    for (const m of migrationFiles(dir)) {
      if (done.has(m.name)) {
        if (done.get(m.name) !== m.checksum) {
          log(`warning: ${m.name} was edited after it was applied. Add a new migration instead of editing an old one.`);
        }
        continue;
      }
      await client.query('BEGIN');
      try {
        await client.query(m.sql);
        await client.query('INSERT INTO schema_migrations (name, checksum) VALUES ($1, $2)', [m.name, m.checksum]);
        await client.query('COMMIT');
      } catch (err) {
        await client.query('ROLLBACK').catch(() => {});
        err.message = `${m.name}: ${err.message}`;
        throw err;
      }
      applied.push(m.name);
      log(`applied ${m.name}`);
    }
    return applied;
  } finally {
    await client.query('SELECT pg_advisory_unlock($1)', [LOCK_ID]).catch(() => {});
  }
}

/** Create the database named in `url` if it doesn't exist (connects to the `postgres` database). */
async function createDatabase(url) {
  const target = new URL(url);
  const name = decodeURIComponent(target.pathname.replace(/^\//, ''));
  if (!name) throw new Error('DATABASE_URL has no database name.');
  const admin = new URL(url);
  admin.pathname = '/postgres';
  const client = new pg.Client({ connectionString: admin.toString() });
  await client.connect();
  try {
    await client.query(`CREATE DATABASE "${name.replace(/"/g, '""')}"`);
    console.log(`[db] created database ${name}`);
  } finally {
    await client.end();
  }
}

function redact(url) {
  try {
    const u = new URL(url);
    if (u.password) u.password = '***';
    return u.toString();
  } catch {
    return '(invalid DATABASE_URL)';
  }
}

async function main() {
  loadEnv();
  const args = new Set(process.argv.slice(2));
  const url = process.env.DATABASE_URL;
  if (!url) {
    console.error('\n  ✖ DATABASE_URL is not set.\n    Run `npm run setup`, then see "Database" in web/README.md.\n');
    process.exit(1);
  }

  const connect = async () => {
    const c = new pg.Client({ connectionString: url, connectionTimeoutMillis: 5_000 });
    await c.connect();
    return c;
  };

  let client;
  try {
    client = await connect();
  } catch (err) {
    if (err.code === '3D000' && args.has('--create')) {
      await createDatabase(url);
      client = await connect();
    } else {
      const why =
        err.code === '3D000' ? 'the database does not exist (run `node scripts/db-migrate.mjs --create`)'
        : err.code === 'ECONNREFUSED' ? 'nothing is listening there. Is Postgres running?'
        : err.message;
      console.error(`\n  ✖ Could not connect to ${redact(url)}: ${why}\n    See "Database" in web/README.md.\n`);
      process.exit(1);
    }
  }

  try {
    if (args.has('--status')) {
      await client.query('CREATE TABLE IF NOT EXISTS schema_migrations (name text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())');
      const done = new Set((await client.query('SELECT name FROM schema_migrations')).rows.map((r) => r.name));
      for (const m of migrationFiles()) console.log(`${done.has(m.name) ? 'applied' : 'pending'}  ${m.name}`);
      return;
    }
    const applied = await migrate(client, { log: (s) => console.log(`[db] ${s}`) });
    if (!applied.length) console.log('[db] schema is up to date');
  } finally {
    await client.end();
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch((err) => {
    console.error('[db] migration failed:', err.message);
    process.exit(1);
  });
}
