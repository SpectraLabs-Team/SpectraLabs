// Delete daily active-time rows older than the retention window (default 400 days).
//   npm run activity:purge                     keep 400 days
//   npm run activity:purge -- --dry-run        only count what would be deleted
//   node scripts/activity-purge.mjs 180        keep 180 days
// The worker runs the same purge every six hours (SPECTRA_ACTIVITY_PURGE, on by default); this
// script is for running it by hand. Both take the same advisory lock, so they never overlap.
// Only user_activity_daily is touched. The audit log is append-only (triggers refuse UPDATE,
// DELETE and TRUNCATE) and is never purged.
import pg from 'pg';
import { loadEnv } from './load-env.mjs';

const DEFAULT_RETENTION_DAYS = 400; // must match ACTIVITY_RETENTION_DAYS in lib/server/activity.ts
const PURGE_LOCK = 'spectra:activity-purge'; // same name as PURGE_LOCK in crates/spectra-db/src/activity.rs

loadEnv();
const args = process.argv.slice(2);
const dryRun = args.includes('--dry-run');
const unknown = args.filter((a) => a.startsWith('--') && a !== '--dry-run');
if (unknown.length) {
  console.error(`Unknown option: ${unknown.join(' ')}. Use --dry-run, or a number of days.`);
  process.exit(2);
}
const days = Number(args.find((a) => !a.startsWith('--')) ?? DEFAULT_RETENTION_DAYS);
if (!Number.isInteger(days) || days < 1) {
  console.error('Retention must be a whole number of days, at least 1.');
  process.exit(2);
}
const client = new pg.Client({ connectionString: process.env.DATABASE_URL });
await client.connect();
try {
  await client.query('BEGIN');
  const { rows: [{ locked }] } = await client.query('SELECT pg_try_advisory_xact_lock(hashtext($1)) AS locked', [PURGE_LOCK]);
  if (!locked) {
    await client.query('ROLLBACK');
    console.log('Another activity purge is running (the worker, or another copy of this script). Nothing done.');
    process.exitCode = 3;
  } else {
    const where = "day < (now() AT TIME ZONE 'UTC')::date - $1::int";
    if (dryRun) {
      const { rows: [{ n }] } = await client.query(`SELECT count(*)::int AS n FROM user_activity_daily WHERE ${where}`, [days]);
      await client.query('ROLLBACK');
      console.log(`Dry run: would remove ${n} daily activity row(s) older than ${days} days. Nothing deleted.`);
    } else {
      const { rowCount } = await client.query(`DELETE FROM user_activity_daily WHERE ${where}`, [days]);
      await client.query('COMMIT');
      console.log(`Removed ${rowCount} daily activity row(s) older than ${days} days.`);
    }
  }
} catch (err) {
  await client.query('ROLLBACK').catch(() => {});
  throw err;
} finally {
  await client.end();
}
