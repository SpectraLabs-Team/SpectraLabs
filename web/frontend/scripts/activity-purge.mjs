// Delete daily active-time rows older than the retention window (default 400 days).
//   npm run activity:purge            keep 400 days
//   node scripts/activity-purge.mjs 180
// Only user_activity_daily is touched. The audit log is append-only and is never purged here.
import pg from 'pg';
import { loadEnv } from './load-env.mjs';

const DEFAULT_RETENTION_DAYS = 400; // must match ACTIVITY_RETENTION_DAYS in lib/server/activity.ts

loadEnv();
const days = Number(process.argv[2] ?? DEFAULT_RETENTION_DAYS);
if (!Number.isInteger(days) || days < 1) {
  console.error('Retention must be a whole number of days, at least 1.');
  process.exit(2);
}
const client = new pg.Client({ connectionString: process.env.DATABASE_URL });
await client.connect();
try {
  const { rowCount } = await client.query("DELETE FROM user_activity_daily WHERE day < (now() AT TIME ZONE 'UTC')::date - $1::int", [days]);
  console.log(`Removed ${rowCount} daily activity row(s) older than ${days} days.`);
} finally {
  await client.end();
}
