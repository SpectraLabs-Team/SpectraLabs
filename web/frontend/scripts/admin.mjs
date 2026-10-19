// Platform admin role, from the command line (the role lives in the database: users.platform_role).
//   npm run admin -- list
//   npm run admin -- grant  someone@example.com
//   npm run admin -- revoke someone@example.com
//
// A role change signs that account out everywhere, so its next sign-in gets a fresh session with
// the new role. Granting admin also turns on two-step login (always on for admins). The last admin
// can't be revoked. ADMIN_EMAIL in .env.local only seeds the very first admin at sign-up.
import pg from 'pg';
import { loadEnv } from './load-env.mjs';

loadEnv();
const [cmd, rawEmail] = process.argv.slice(2);
const email = (rawEmail || '').trim().toLowerCase();
const usage = 'Usage: npm run admin -- list | grant <email> | revoke <email>';

if (!['list', 'grant', 'revoke'].includes(cmd) || (cmd !== 'list' && !email)) {
  console.error(usage);
  process.exit(1);
}
if (!process.env.DATABASE_URL) {
  console.error('✖ DATABASE_URL is not set. Run `npm run setup` first.');
  process.exit(1);
}

const client = new pg.Client({ connectionString: process.env.DATABASE_URL });
await client.connect();
try {
  if (cmd === 'list') {
    const { rows } = await client.query(`SELECT email, name, created_at FROM users WHERE platform_role = 'admin' ORDER BY created_at`);
    if (!rows.length) console.log('No admins yet. Grant one with: npm run admin -- grant <email>');
    for (const r of rows) console.log(`  ${r.email}${r.name ? ` (${r.name})` : ''}`);
  } else {
    const role = cmd === 'grant' ? 'admin' : 'user';
    await client.query('BEGIN');
    await client.query('SELECT pg_advisory_xact_lock(7312020)'); // same lock as first-admin seeding
    const { rows } = await client.query('SELECT id, platform_role FROM users WHERE email = $1 FOR UPDATE', [email]);
    if (!rows.length) {
      await client.query('ROLLBACK');
      console.error(`✖ No account for ${email}. They need to sign up first.`);
      process.exit(1);
    }
    const user = rows[0];
    if (user.platform_role === role) {
      await client.query('ROLLBACK');
      console.log(`${email} is already ${role === 'admin' ? 'an admin' : 'not an admin'}. Nothing changed.`);
    } else {
      if (role === 'user') {
        const { rows: [{ n }] } = await client.query(`SELECT count(*)::int AS n FROM users WHERE platform_role = 'admin'`);
        if (n <= 1) {
          await client.query('ROLLBACK');
          console.error('✖ That is the only admin. Grant admin to someone else first.');
          process.exit(1);
        }
      }
      await client.query(`UPDATE users SET platform_role = $2, two_factor = (two_factor OR $2 = 'admin') WHERE id = $1`, [user.id, role]);
      const ended = await client.query('UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL', [user.id]);
      // Audit trail (same table as the app's), in the same transaction: no change without a record.
      const operator = `cli:${process.env.USER || process.env.USERNAME || 'unknown'}`;
      await client.query(
        `INSERT INTO audit_events (actor_email, action, target_type, target_id, details) VALUES ($1, $2, 'user', $3, $4::jsonb)`,
        [operator, role === 'admin' ? 'admin.role.granted' : 'admin.role.revoked', user.id, JSON.stringify({ email, sessionsRevoked: ended.rowCount, via: 'npm run admin' })],
      );
      await client.query('COMMIT');
      console.log(`${email} is now ${role === 'admin' ? 'an admin (two-step login turned on)' : 'a regular user'}. Signed out of ${ended.rowCount} session(s).`);
    }
  }
} catch (err) {
  await client.query('ROLLBACK').catch(() => {});
  console.error('✖', err.message);
  process.exitCode = 1;
} finally {
  await client.end();
}
