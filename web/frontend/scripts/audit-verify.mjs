// Check the audit log's hash chain (M19 task 9).   npm run audit:verify
// Exit code 1 if any row was altered or removed. Prints the head hash: keep a copy somewhere else
// (a ticket, a chat message) now and then, so truncating the newest rows would be noticed too.
import pg from 'pg';
import { loadEnv } from './load-env.mjs';

loadEnv();
if (!process.env.DATABASE_URL) {
  console.error('✖ DATABASE_URL is not set. Run `npm run setup` first.');
  process.exit(1);
}
const client = new pg.Client({ connectionString: process.env.DATABASE_URL });
await client.connect();
try {
  const breaks = (await client.query('SELECT seq::text, problem FROM audit_chain_breaks() LIMIT 50')).rows;
  const { rows: [stat] } = await client.query(
    `SELECT count(*)::int AS n, (SELECT encode(hash, 'hex') FROM audit_events ORDER BY seq DESC LIMIT 1) AS head,
            (SELECT max(seq)::text FROM audit_events) AS last_seq FROM audit_events`,
  );
  if (breaks.length) {
    console.error(`✖ Audit chain BROKEN at ${breaks.length}${breaks.length === 50 ? '+' : ''} row(s):`);
    for (const b of breaks) console.error(`    seq ${b.seq}: ${b.problem}`);
    process.exitCode = 1;
  } else {
    console.log(`✔ Audit chain intact: ${stat.n} event(s).`);
  }
  if (stat.head) console.log(`  head: seq ${stat.last_seq} ${stat.head}`);
} finally {
  await client.end();
}
