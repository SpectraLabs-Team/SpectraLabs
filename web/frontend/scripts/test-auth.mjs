// Auth & security tests. Run: npm test
//
// Needs a Postgres server. Uses TEST_DATABASE_URL if set, otherwise DATABASE_URL (shell or
// .env.local). Every run creates its own throwaway schema (spectra_test_<random>), applies the
// migrations there, and drops it at the end, so it never touches your real tables.
//
// Uses Node's built-in test runner + the project's TypeScript compiler (no test framework).
import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { createRequire } from 'node:module';
import pg from 'pg';
import { loadEnv, root } from './load-env.mjs';
import { migrate } from './db-migrate.mjs';
import { toRow, importRows } from './import-users.mjs';

const require = createRequire(import.meta.url);
const ts = require(path.join(root, 'node_modules/typescript'));

// ── Test database: a fresh schema per run ──
loadEnv();
const baseUrl = process.env.TEST_DATABASE_URL || process.env.DATABASE_URL;
if (!baseUrl) {
  console.error('\n  ✖ No database for tests. Set TEST_DATABASE_URL (or DATABASE_URL in .env.local). See web/README.md.\n');
  process.exit(1);
}
const schema = `spectra_test_${crypto.randomBytes(4).toString('hex')}`;
const withSearchPath = (url) => `${url}${url.includes('?') ? '&' : '?'}options=${encodeURIComponent(`-c search_path=${schema}`)}`;
const testUrl = withSearchPath(baseUrl);

const admin = new pg.Client({ connectionString: baseUrl });
try {
  await admin.connect();
} catch (err) {
  console.error(`\n  ✖ Could not connect to the test database: ${err.message}\n    Is Postgres running? See web/README.md.\n`);
  process.exit(1);
}
await admin.query(`CREATE SCHEMA ${schema}`);
const sql = new pg.Client({ connectionString: testUrl }); // direct access for assertions
await sql.connect();
await migrate(sql);

process.env.DATABASE_URL = testUrl;
process.env.AUTH_SECRET = 'test-secret-' + crypto.randomBytes(16).toString('hex');
process.env.ADMIN_EMAIL = 'admin@x.com'; // seeds the first admin
const MASTER_V1 = crypto.randomBytes(32).toString('base64');
process.env.SECRETS_MASTER_KEY = MASTER_V1;

// ── Load the TypeScript modules under test (transpiled to CommonJS, directory layout kept so
// relative imports like './db' resolve; inside node_modules so 'pg' resolves too) ──
const build = path.join(root, 'node_modules', '.cache', 'spectra-auth-test');
fs.rmSync(build, { recursive: true, force: true });
fs.mkdirSync(build, { recursive: true });
fs.writeFileSync(path.join(build, 'package.json'), '{"type":"commonjs"}');
for (const rel of ['lib/auth-shared.ts', 'lib/server/db.ts', 'lib/server/authStore.ts', 'lib/server/sessions.ts', 'lib/server/workspaces.ts', 'lib/server/engineIdentity.ts', 'lib/server/authz.ts', 'lib/server/engineRoutes.ts', 'lib/server/lockout.ts', 'lib/server/vault.ts', 'lib/server/audit.ts', 'lib/server/totp.ts', 'lib/server/mfa.ts', 'lib/userAgent.ts', 'lib/server/devices.ts', 'lib/server/team.ts', 'lib/server/appUrl.ts', 'lib/server/adminUsers.ts', 'lib/csp.ts', 'lib/sessionEdge.ts', 'lib/server/rateLimit.ts', 'lib/server/breached.ts', 'lib/server/qr.ts', 'lib/server/spikes.ts', 'lib/server/apiTokens.ts', 'lib/server/engineSign.ts', 'lib/server/engineUpload.ts', 'lib/api.ts', 'lib/jobs.ts', 'lib/datasets.ts']) {
  const { outputText } = ts.transpileModule(fs.readFileSync(path.join(root, rel), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, esModuleInterop: true },
  });
  const out = path.join(build, rel.replace(/\.tsx?$/, '.js'));
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, outputText);
}
const load = (rel) => require(path.join(build, rel));

const shared = load('lib/auth-shared.js');
const edge = load('lib/sessionEdge.js');
let rl = load('lib/server/rateLimit.js');
let db = load('lib/server/db.js');
let store = load('lib/server/authStore.js');
let sessions = load('lib/server/sessions.js');
let workspaces = load('lib/server/workspaces.js');
const engineId = load('lib/server/engineIdentity.js');
const engineSign = load('lib/server/engineSign.js');
const engineUpload = load('lib/server/engineUpload.js');
const apiClient = load('lib/api.js');
const datasetsClient = load('lib/datasets.js');
let authz = load('lib/server/authz.js');
let lockout = load('lib/server/lockout.js');
let vault = load('lib/server/vault.js');
let auditLog = load('lib/server/audit.js');
const totp = load('lib/server/totp.js');
let mfa = load('lib/server/mfa.js');
const ua_ = load('lib/userAgent.js');
let devices = load('lib/server/devices.js');
let team = load('lib/server/team.js');
const appUrlMod = load('lib/server/appUrl.js');
let adminUsers = load('lib/server/adminUsers.js');
const csp = load('lib/csp.js');
const engineRoutes = load('lib/server/engineRoutes.js');
let breached = load('lib/server/breached.js');
const qr = load('lib/server/qr.js');

/** Simulate a server restart: close the pool and load fresh module instances (no in-memory state survives). */
async function restart() {
  await db.closePool();
  for (const k of Object.keys(require.cache)) if (k.startsWith(build)) delete require.cache[k];
  db = load('lib/server/db.js');
  store = load('lib/server/authStore.js');
  sessions = load('lib/server/sessions.js');
  workspaces = load('lib/server/workspaces.js');
  authz = load('lib/server/authz.js');
  rl = load('lib/server/rateLimit.js');
  lockout = load('lib/server/lockout.js');
  vault = load('lib/server/vault.js');
  auditLog = load('lib/server/audit.js');
  mfa = load('lib/server/mfa.js');
  devices = load('lib/server/devices.js');
  team = load('lib/server/team.js');
  adminUsers = load('lib/server/adminUsers.js');
  breached = load('lib/server/breached.js');
}


after(async () => {
  await db.closePool();
  await sql.end();
  await admin.query(`DROP SCHEMA ${schema} CASCADE`);
  await admin.end();
  process.chdir(root);
  fs.rmSync(build, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});


test('display helpers', () => {
  assert.equal(shared.initials({ email: 'x@y.z', name: 'Amogh R Gowda' }), 'AG');
  assert.equal(shared.initials({ email: 'alex.vance@fund.com', name: '' }), 'AV');
  assert.equal(shared.displayName({ email: 'alex.vance@fund.com' }), 'Alex Vance');
  assert.equal(shared.cleanName('  Alex   Vance '), 'Alex Vance');
  assert.equal(shared.cleanName('A'), '');
});

test('password rules', () => {
  assert.equal(shared.isPasswordValid('Passw0rd!'), true);
  for (const bad of ['short1!', 'NoDigits!!', 'NoSpecial12', '12345678!']) assert.equal(shared.isPasswordValid(bad), false, bad);
  assert.equal(shared.isPasswordValid('Aa1!' + 'x'.repeat(252)), true, '256 characters is the maximum');
  assert.equal(shared.isPasswordValid('Aa1!' + 'x'.repeat(253)), false, 'over 256 is a clear 400, not a hashing error');
});

test('migrations: tables exist, re-running is a no-op', async () => {
  const tables = (await sql.query(`SELECT table_name FROM information_schema.tables WHERE table_schema = $1 ORDER BY 1`, [schema])).rows.map((r) => r.table_name);
  assert.deepEqual(tables, ['api_tokens', 'audit_events', 'email_codes', 'known_devices', 'rate_limits', 'recovery_codes', 'schema_migrations', 'secrets', 'sessions', 'user_totp', 'users', 'workspace_invites', 'workspace_keys', 'workspace_members', 'workspaces']);
  assert.deepEqual(await migrate(sql), [], 'nothing left to apply');
});

test('sign-up codes are hashed at rest, single-use, and lock after 5 misses', async () => {
  await store.setPendingOtp('new@x.com', '123456', await store.hashPassword('Passw0rd!'), 'New User');
  const rows = (await sql.query(`SELECT * FROM email_codes WHERE email = 'new@x.com'`)).rows;
  assert.equal(rows.length, 1);
  assert.ok(!JSON.stringify(rows).includes('123456'), 'plaintext code must not be stored');

  assert.equal((await store.checkSignupCode('new@x.com', '000000')).result, 'wrong');
  const ok = await store.checkSignupCode('new@x.com', '123456');
  assert.equal(ok.result, 'ok');
  assert.equal(ok.entry.name, 'New User');
  assert.equal((await store.checkSignupCode('new@x.com', '123456')).result, 'missing', 'code is single-use');

  await store.setPendingOtp('lock@x.com', '111111', 'h', '');
  for (let i = 0; i < 5; i++) assert.equal((await store.checkSignupCode('lock@x.com', '999999')).result, 'wrong');
  assert.equal((await store.checkSignupCode('lock@x.com', '111111')).result, 'locked', 'right code after 5 misses is refused');
  assert.equal((await store.checkSignupCode('lock@x.com', '111111')).result, 'missing', 'a locked code is deleted');
});

test('a new code replaces the old one and resets the attempt count', async () => {
  await store.setResetCode('again@x.com', '100001');
  for (let i = 0; i < 3; i++) await store.checkResetCode('again@x.com', '000000');
  await store.setResetCode('again@x.com', '100002');
  assert.equal((await store.checkResetCode('again@x.com', '100001')).result, 'wrong', 'old code is gone');
  assert.equal((await store.checkResetCode('again@x.com', '100002')).result, 'ok');
});

test('expired codes are refused', async () => {
  await store.setLoginCode('late@x.com', '555555');
  await sql.query(`UPDATE email_codes SET expires_at = now() - interval '1 second' WHERE email = 'late@x.com'`);
  assert.equal((await store.checkLoginCode('late@x.com', '555555')).result, 'missing');
});

test('a code can be redeemed only once, even by parallel requests', async () => {
  await store.setPendingOtp('race@x.com', '246810', 'h', 'Racer');
  const results = await Promise.all(Array.from({ length: 5 }, () => store.checkSignupCode('race@x.com', '246810')));
  assert.equal(results.filter((r) => r.result === 'ok').length, 1, 'exactly one winner');
});

test('sign-up and reset codes cannot be swapped', async () => {
  await store.setResetCode('swap@x.com', '222222');
  assert.equal((await store.checkSignupCode('swap@x.com', '222222')).result, 'missing');
  assert.equal((await store.checkResetCode('swap@x.com', '222222')).result, 'ok');
});

test('emails are trimmed and lower-cased; the database enforces it', async () => {
  const u = await store.createUser('  Mixed.Case@X.com ', 'h', 'M');
  assert.equal(u.email, 'mixed.case@x.com');
  assert.ok(await store.findUser('MIXED.CASE@x.COM'));
  await assert.rejects(sql.query(`INSERT INTO users (email, password_hash) VALUES ('Upper@X.com', 'h')`), /check constraint/);
  const again = await store.createUser('mixed.case@x.com', 'other', 'Dup');
  assert.equal(again.id, u.id, 'creating an existing email returns the existing account');
  assert.equal(again.passwordHash, 'h', '...unchanged');
});

const newSession = (user, meta = {}) => sessions.createSession(user, { ip: '203.0.113.7', userAgent: 'TestBrowser/1.0', ...meta });
const lookupEmail = async (token) => (await sessions.lookupSession(token))?.user.email ?? null;

test('sessions: opaque random token, only a keyed hash stored, lookup returns the live account', async () => {
  const u = await store.createUser('amogh@x.com', await store.hashPassword('Passw0rd!'), 'Amogh');
  const { token, session } = await newSession(u);
  assert.match(token, /^[A-Za-z0-9_-]{43}$/, '32 random bytes, base64url');
  assert.ok(edge.looksLikeSessionToken(token));

  const row = (await sql.query('SELECT * FROM sessions WHERE id = $1', [session.id])).rows[0];
  assert.ok(!JSON.stringify(row).includes(token), 'token is never stored');
  assert.notEqual(row.token_hash.toString('hex'), crypto.createHash('sha256').update(token).digest('hex'), 'hash is keyed with AUTH_SECRET');
  assert.equal(row.ip, '203.0.113.7');
  assert.equal(row.user_agent, 'TestBrowser/1.0');
  const lifetime = row.expires_at - row.created_at;
  assert.ok(Math.abs(lifetime - 7 * 24 * 3600_000) < 5000, '7-day absolute lifetime');

  const found = await sessions.lookupSession(token);
  assert.equal(found.user.email, 'amogh@x.com');
  assert.equal(found.session.id, session.id);
  assert.equal(found.user.name, 'Amogh');

  assert.equal(await sessions.lookupSession(crypto.randomBytes(32).toString('base64url')), null, 'unknown token');
  assert.equal(await sessions.lookupSession(token.slice(0, -1) + (token.endsWith('A') ? 'B' : 'A')), null, 'altered token');
  assert.equal(await sessions.lookupSession('not a token'), null);
  assert.equal(await sessions.lookupSession(undefined), null);
});

test('sessions: idle timeout (24h), absolute expiry, last_seen_at refreshed at most once a minute', async () => {
  const u = await store.createUser('idle@x.com', 'h', 'Idle');
  const { token, session } = await newSession(u);

  await sql.query(`UPDATE sessions SET last_seen_at = now() - interval '30 seconds' WHERE id = $1`, [session.id]);
  const before = (await sql.query('SELECT last_seen_at FROM sessions WHERE id = $1', [session.id])).rows[0].last_seen_at;
  assert.ok(await sessions.lookupSession(token));
  const unchanged = (await sql.query('SELECT last_seen_at FROM sessions WHERE id = $1', [session.id])).rows[0].last_seen_at;
  assert.equal(unchanged.getTime(), before.getTime(), 'no write within a minute');

  await sql.query(`UPDATE sessions SET last_seen_at = now() - interval '23 hours' WHERE id = $1`, [session.id]);
  assert.ok(await sessions.lookupSession(token), 'still alive after 23h idle');
  const touched = (await sql.query('SELECT last_seen_at FROM sessions WHERE id = $1', [session.id])).rows[0].last_seen_at;
  assert.ok(Date.now() - touched.getTime() < 10_000, 'use refreshes last_seen_at');

  await sql.query(`UPDATE sessions SET last_seen_at = now() - interval '25 hours' WHERE id = $1`, [session.id]);
  assert.equal(await sessions.lookupSession(token), null, 'dead after 24h idle');

  const second = await newSession(u);
  await sql.query(`UPDATE sessions SET expires_at = now() - interval '1 second' WHERE id = $1`, [second.session.id]);
  assert.equal(await sessions.lookupSession(second.token), null, 'dead after absolute expiry, however active');
});

test('sessions: rotation on sign-in, sign out one, sign out everywhere else, device list', async () => {
  const u = await store.createUser('devices@x.com', 'h', 'D');
  const planted = await newSession(u);
  const rotated = await newSession(u, { replacing: planted.token });
  assert.equal(await sessions.lookupSession(planted.token), null, 'signing in again revokes the browser\'s previous token');
  assert.ok(await sessions.lookupSession(rotated.token));

  const laptop = await newSession(u, { userAgent: 'Laptop' });
  const phone = await newSession(u, { userAgent: 'Phone' });
  const list = await sessions.listSessions(u.id);
  assert.deepEqual(new Set(list.map((x) => x.id)), new Set([rotated.session.id, laptop.session.id, phone.session.id]), 'revoked sessions are not listed');

  const other = await store.createUser('stranger@x.com', 'h', 'S');
  assert.equal(await sessions.revokeSessionById(other.id, phone.session.id), false, 'cannot end another account\'s session');
  assert.equal(await sessions.revokeSessionById(u.id, phone.session.id), true);
  assert.equal(await sessions.lookupSession(phone.token), null);
  assert.equal(await sessions.revokeSessionById(u.id, phone.session.id), false, 'already ended');

  assert.equal(await sessions.revokeAllSessions(u.id, { except: laptop.session.id }), 1);
  assert.equal(await sessions.lookupSession(rotated.token), null, 'other devices signed out');
  assert.ok(await sessions.lookupSession(laptop.token), 'this device stays signed in');

  await sessions.revokeSessionByToken(laptop.token); // Sign out
  assert.equal(await sessions.lookupSession(laptop.token), null);
  assert.deepEqual(await sessions.listSessions(u.id), []);
});

test('password change or reset signs out every session', async () => {
  const u = await store.createUser('reset@x.com', await store.hashPassword('OldPassw0rd!'), 'R');
  const a = await newSession(u);
  const b = await newSession(u);
  await store.updatePassword('reset@x.com', await store.hashPassword('NewPassw0rd!'));
  assert.equal(await sessions.lookupSession(a.token), null, 'pre-reset sessions must die');
  assert.equal(await sessions.lookupSession(b.token), null);
  const fresh = await newSession(await store.findUser('reset@x.com'));
  assert.ok(await sessions.lookupSession(fresh.token), 'post-reset session works');
  assert.ok((await store.verifyPassword('NewPassw0rd!', (await store.findUser('reset@x.com')).passwordHash)).ok);
  assert.ok(!(await store.verifyPassword('OldPassw0rd!', (await store.findUser('reset@x.com')).passwordHash)).ok);
});

test('disabled accounts lose their sessions', async () => {
  const u = await store.createUser('off@x.com', 'h', 'Off');
  const { token } = await newSession(u);
  assert.ok(await sessions.lookupSession(token));
  await sql.query(`UPDATE users SET status = 'disabled' WHERE email = 'off@x.com'`);
  assert.equal(await sessions.lookupSession(token), null);
  assert.equal((await store.findUser('off@x.com')).status, 'disabled');
});

test('admin listing has no password hashes', async () => {
  const list = await store.listUsers();
  assert.ok(list.length > 0);
  assert.ok(list.every((x) => !('passwordHash' in x) && !('password_hash' in x)));
});

test('middleware (Edge) check: only the token shape, old signed cookies rejected', () => {
  assert.equal(edge.looksLikeSessionToken(crypto.randomBytes(32).toString('base64url')), true);
  const oldSigned = Buffer.from('{"email":"a@b.c","role":"user"}').toString('base64url') + '.' + 'x'.repeat(43);
  assert.equal(edge.looksLikeSessionToken(oldSigned), false, 'pre-task-2 cookies are cleared by middleware');
  for (const bad of [undefined, null, '', 'short', 'a'.repeat(44), 'a'.repeat(42) + '!']) assert.equal(edge.looksLikeSessionToken(bad), false);
});

test('2FA: per-account setting, admin always on, login codes isolated', async () => {
  const on = await store.createUser('twofa@x.com', 'h', 'T'); // default for new accounts
  assert.equal(store.twoFactorOn(on), true);
  const off = await store.createUser('onefa@x.com', 'h', 'O', false);
  assert.equal(store.twoFactorOn(off), false);
  await store.setTwoFactor('onefa@x.com', true);
  assert.equal(store.twoFactorOn(await store.findUser('onefa@x.com')), true);

  const admin = await store.createUser('admin@x.com', 'h', 'Admin', false); // asks for off...
  assert.equal(admin.role, 'admin');
  assert.equal(store.twoFactorOn(admin), true, '...but admins are always 2FA');
  await store.setTwoFactor('admin@x.com', false);
  assert.equal((await store.findUser('admin@x.com')).twoFactor, true, 'admin cannot switch it off (stored value too)');

  await store.setLoginCode('twofa@x.com', '333333');
  assert.equal((await store.checkSignupCode('twofa@x.com', '333333')).result, 'missing', 'login code is not a sign-up code');
  assert.equal((await store.checkResetCode('twofa@x.com', '333333')).result, 'missing', 'login code is not a reset code');
  assert.equal((await store.checkLoginCode('twofa@x.com', '333333')).result, 'ok');
  assert.equal((await store.checkLoginCode('twofa@x.com', '333333')).result, 'missing', 'single-use');
  assert.equal((await store.listUsers()).find((u) => u.email === 'twofa@x.com').twoFactor, true);
});

test('account: rename shows live, delete removes sessions and codes', async () => {
  const u = await store.createUser('acct@x.com', await store.hashPassword('Passw0rd!'), 'Old Name');
  const { token } = await newSession(u);
  await store.updateName('acct@x.com', 'New Name');
  assert.equal((await sessions.lookupSession(token)).user.name, 'New Name', 'name is read live, so the header updates without re-login');

  await store.setLoginCode('acct@x.com', '444444');
  assert.equal(await store.deleteUser('acct@x.com'), true);
  assert.equal(await store.findUser('acct@x.com'), undefined);
  assert.equal(await sessions.lookupSession(token), null, 'deleted account → no session');
  assert.equal((await sql.query('SELECT count(*)::int AS n FROM sessions WHERE user_id = $1', [u.id])).rows[0].n, 0, 'session rows deleted with the account');
  assert.equal((await store.checkLoginCode('acct@x.com', '444444')).result, 'missing', 'pending codes are removed too');
  assert.ok(!(await store.listUsers()).some((x) => x.email === 'acct@x.com'));
  assert.equal(await store.deleteUser('acct@x.com'), false);
});

// "Done when" for M19 task 1: every flow works with the server restarted between steps.
test('sign-up, two-step login, sessions and reset survive a restart between every step', async () => {
  const email = 'restart@x.com';
  const password = 'Passw0rd!';

  // Sign-up: request a code (send-otp) ... restart ... confirm it (verify-otp).
  await store.setPendingOtp(email, '700001', await store.hashPassword(password), 'Restart Tester', true);
  await restart();
  assert.ok(await store.getPendingOtp(email), 'pending sign-up survives (resend still works)');
  const { result, entry } = await store.checkSignupCode(email, '700001');
  assert.equal(result, 'ok');
  const created = await store.createUser(email, entry.passwordHash, entry.name, entry.twoFactor !== false);
  const { token: signupToken } = await newSession(created);
  await restart();
  assert.equal(await lookupEmail(signupToken), email, 'session from sign-up still valid');

  // Two-step login: password + code issued ... restart ... code confirmed.
  const user = await store.findUser(email);
  assert.ok((await store.verifyPassword(password, user.passwordHash)).ok);
  assert.equal(store.twoFactorOn(user), true);
  await store.setLoginCode(email, '700002');
  await restart();
  assert.equal((await store.checkLoginCode(email, '700002')).result, 'ok');
  const { token: loginToken } = await newSession(await store.findUser(email), { replacing: signupToken });
  await restart();
  assert.equal(await lookupEmail(loginToken), email);
  assert.equal(await lookupEmail(signupToken), null, 'login rotated the old token');

  // Reset: code issued ... restart ... code + new password ... restart ... old session dead, new password works.
  await store.setResetCode(email, '700003');
  await restart();
  assert.equal((await store.checkResetCode(email, '700003')).result, 'ok');
  await store.updatePassword(email, await store.hashPassword('N3wPassw0rd!'));
  await restart();
  assert.equal(await lookupEmail(loginToken), null, 'pre-reset session revoked');
  const after = await store.findUser(email);
  assert.ok((await store.verifyPassword('N3wPassw0rd!', after.passwordHash)).ok);
  assert.ok(!(await store.verifyPassword(password, after.passwordHash)).ok);
});

test('import from users.json: keeps hashes, roles and times; idempotent; skips bad records', async () => {
  const hash = await store.hashPassword('Imp0rted!');
  const file = [
    { email: ' Old.User@X.com', name: 'Old User', passwordHash: hash, role: 'user', createdAt: 1_700_000_000_000, twoFactor: false, passwordChangedAt: 1_700_000_100_000 },
    { email: 'old.admin@x.com', name: 'Old Admin', passwordHash: hash, role: 'admin', createdAt: 1_700_000_200_000, twoFactor: false },
    { email: 'legacy@x.com', passwordHash: hash, role: 'user', createdAt: 1_700_000_300_000 }, // predates the 2FA field
    { email: 'not-an-email', passwordHash: hash },
    { email: 'nohash@x.com' },
  ];
  const rows = file.map(toRow).filter(([r]) => r).map(([r]) => r);
  assert.equal(rows.length, 3, 'malformed records are skipped');

  const first = await importRows(sql, rows);
  assert.deepEqual(first.imported.sort(), ['legacy@x.com', 'old.admin@x.com', 'old.user@x.com']);
  const second = await importRows(sql, rows);
  assert.equal(second.imported.length, 0, 'second run imports nothing');
  assert.equal(second.skipped.length, 3);

  const u = await store.findUser('old.user@x.com');
  assert.ok((await store.verifyPassword('Imp0rted!', u.passwordHash)).ok, 'existing password still works');
  assert.equal(u.createdAt, 1_700_000_000_000);
  assert.equal(u.passwordChangedAt, 1_700_000_100_000);
  assert.equal(u.twoFactor, false);
  const a = await store.findUser('old.admin@x.com');
  assert.equal(a.role, 'admin');
  assert.equal(store.twoFactorOn(a), true, 'admins always get 2FA');
  assert.equal((await store.findUser('legacy@x.com')).twoFactor, true, 'old records default to 2FA on');

  assert.equal(await lookupEmail((await newSession(u)).token), 'old.user@x.com', 'imported accounts can sign in');
  for (const e of ['old.user@x.com', 'old.admin@x.com', 'legacy@x.com']) {
    assert.equal((await personalOf((await store.findUser(e)).id)).length, 1, `${e} has a personal workspace`);
  }
});

test('AUTH_SECRET: no fallback; missing, short or retired secrets fail closed', async () => {
  const good = process.env.AUTH_SECRET;
  const u = await store.findUser('amogh@x.com');
  const { token } = await newSession(u);
  try {
    for (const bad of [undefined, '', 'short-secret', 'spectralabs_jwt_default_secret_key_32bytes_required']) {
      if (bad === undefined) delete process.env.AUTH_SECRET;
      else process.env.AUTH_SECRET = bad;
      await assert.rejects(newSession(u), /AUTH_SECRET/, `no session can start with ${JSON.stringify(bad)}`);
      assert.equal(await sessions.lookupSession(token), null, 'lookups fail closed');
    }
  } finally {
    process.env.AUTH_SECRET = good;
  }
  assert.ok(await sessions.lookupSession(token), 'works again with the real secret');
  process.env.AUTH_SECRET = 'another-secret-' + crypto.randomBytes(16).toString('hex');
  assert.equal(await sessions.lookupSession(token), null, 'changing AUTH_SECRET signs everyone out');
  process.env.AUTH_SECRET = good;
});

const personalOf = async (userId) => (await sql.query('SELECT * FROM workspaces WHERE personal_owner_id = $1', [userId])).rows;

test('workspaces: every account gets one personal workspace, as owner', async () => {
  const u = await store.createUser('ws-owner@x.com', 'h', 'Owner');
  const personal = await personalOf(u.id);
  assert.equal(personal.length, 1);
  assert.equal(personal[0].name, 'Personal');
  assert.match(personal[0].slug, /^ws-owner-[0-9a-f]{6}$/);
  const list = await workspaces.listWorkspaces(u.id);
  assert.deepEqual(list.map((w) => [w.personal, w.role]), [[true, 'owner']]);
  await store.createUser('ws-owner@x.com', 'h', 'Again'); // existing account → no second workspace
  assert.equal((await personalOf(u.id)).length, 1);
  await assert.rejects(
    sql.query(`INSERT INTO workspaces (name, slug, personal_owner_id) VALUES ('Second', 'second-aaaaaa', $1)`, [u.id]),
    /duplicate key/,
    'the database allows only one personal workspace per user',
  );
});

test('workspaces: create a team, list order, names and slugs', async () => {
  const u = await store.findUser('ws-owner@x.com');
  const zeta = await workspaces.createWorkspace(u.id, 'Zeta Fund');
  const alpha = await workspaces.createWorkspace(u.id, 'alpha desk');
  assert.equal(zeta.role, 'owner');
  assert.equal(zeta.personal, false);
  assert.match(zeta.slug, /^zeta-fund-[0-9a-f]{6}$/);
  assert.deepEqual((await workspaces.listWorkspaces(u.id)).map((w) => w.name), ['Personal', 'alpha desk', 'Zeta Fund'], 'personal first, then by name');
  assert.equal(workspaces.cleanWorkspaceName('  Big   Desk '), 'Big Desk');
  assert.equal(workspaces.cleanWorkspaceName('   '), '');
  assert.equal(workspaces.cleanWorkspaceName('x'.repeat(61)), '');
  assert.match(workspaces.makeSlug('!!!'), /^workspace-[0-9a-f]{6}$/);
  assert.ok(alpha.id && zeta.id !== alpha.id);
});

test('sessions carry the active workspace; switching needs membership; access is re-checked every request', async () => {
  const owner = await store.findUser('ws-owner@x.com');
  const member = await store.createUser('ws-member@x.com', 'h', 'Member');
  const team = await workspaces.createWorkspace(owner.id, 'Shared Desk');
  await sql.query(`INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'viewer')`, [team.id, member.id]);

  const { token, session } = await newSession(member);
  let auth = await sessions.lookupSession(token);
  assert.equal(auth.workspace.personal, true, 'a first session starts in the personal workspace');
  assert.equal(auth.workspace.role, 'owner');

  const stranger = await store.createUser('ws-stranger@x.com', 'h', 'S');
  const strangerTeam = await workspaces.createWorkspace(stranger.id, 'Private');
  assert.equal(await workspaces.setActiveWorkspace(session.id, member.id, strangerTeam.id), null, 'cannot switch into a workspace you are not in');
  assert.equal(await workspaces.setActiveWorkspace(session.id, member.id, 'not-a-uuid'), null);

  const switched = await workspaces.setActiveWorkspace(session.id, member.id, team.id);
  assert.equal(switched.role, 'viewer');
  auth = await sessions.lookupSession(token);
  assert.equal(auth.workspace.id, team.id);
  assert.equal(auth.workspace.role, 'viewer', 'role comes from the membership');

  const next = await newSession(member);
  assert.equal((await sessions.lookupSession(next.token)).workspace.id, team.id, 'a new sign-in lands where the user left off');

  await sql.query(`UPDATE workspace_members SET role = 'editor' WHERE workspace_id = $1 AND user_id = $2`, [team.id, member.id]);
  assert.equal((await sessions.lookupSession(token)).workspace.role, 'editor', 'role changes apply on the next request');

  await sql.query('DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2', [team.id, member.id]);
  auth = await sessions.lookupSession(token);
  assert.equal(auth.workspace.personal, true, 'removed member falls back to personal at once');
  assert.equal((await sql.query('SELECT workspace_id FROM sessions WHERE id = $1', [session.id])).rows[0].workspace_id, auth.workspace.id, '...and the session row is updated');

  await sql.query(`INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'editor')`, [team.id, member.id]);
  await workspaces.setActiveWorkspace(session.id, member.id, team.id);
  await sql.query('DELETE FROM workspaces WHERE id = $1', [team.id]);
  assert.equal((await sessions.lookupSession(token)).workspace.personal, true, 'deleted workspace → personal');
});

test('deleting an account: personal and solo teams go with it; sole owner of a shared team is refused', async () => {
  const lead = await store.createUser('ws-lead@x.com', 'h', 'Lead');
  const helper = await store.createUser('ws-helper@x.com', 'h', 'Helper');
  const solo = await workspaces.createWorkspace(lead.id, 'Solo Lab');
  const shared = await workspaces.createWorkspace(lead.id, 'Shared Lab');
  await sql.query(`INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'editor')`, [shared.id, helper.id]);

  await assert.rejects(store.deleteUser('ws-lead@x.com'), (err) => err.name === 'SoleOwnerError' && /Shared Lab/.test(err.message));
  assert.ok(await store.findUser('ws-lead@x.com'), 'nothing was deleted');
  assert.equal((await sql.query('SELECT count(*)::int AS n FROM workspaces WHERE id = $1', [solo.id])).rows[0].n, 1, 'rolled back');

  await sql.query(`UPDATE workspace_members SET role = 'owner' WHERE workspace_id = $1 AND user_id = $2`, [shared.id, helper.id]);
  assert.equal(await store.deleteUser('ws-lead@x.com'), true);
  const left = (await sql.query('SELECT id FROM workspaces WHERE id = ANY($1)', [[solo.id, shared.id]])).rows.map((r) => r.id);
  assert.deepEqual(left, [shared.id], 'solo team deleted, shared team kept with its other owner');
  assert.deepEqual(await personalOf(lead.id), [], 'personal workspace deleted');
});

test('engine identity header: signed, scoped to method + path, short-lived', () => {
  const secret = 'engine-secret-' + 'x'.repeat(32);
  const id = { userId: 'u1', sessionId: 's1', workspaceId: 'w1', workspaceRole: 'editor', platformRole: 'user', method: 'post', path: 'api/risk' };
  const prev = process.env.ENGINE_SHARED_SECRET;
  try {
    delete process.env.ENGINE_SHARED_SECRET;
    assert.equal(engineId.signEngineIdentity(id), null, 'no secret → no header');
    process.env.ENGINE_SHARED_SECRET = secret;
    const now = Date.now();
    const header = engineId.signEngineIdentity(id, now);
    const data = engineId.verifyEngineIdentity(header, { method: 'POST', path: 'api/risk' }, secret, now);
    assert.equal(data.wid, 'w1');
    assert.equal(data.wrole, 'editor');
    assert.equal(data.uid, 'u1');
    assert.equal(engineId.verifyEngineIdentity(header, { method: 'POST', path: 'api/stress' }, secret, now), null, 'other route');
    assert.equal(engineId.verifyEngineIdentity(header, { method: 'GET', path: 'api/risk' }, secret, now), null, 'other method');
    assert.equal(engineId.verifyEngineIdentity(header, { method: 'POST', path: 'api/risk' }, 'wrong-secret-' + 'y'.repeat(32), now), null, 'wrong secret');
    assert.equal(engineId.verifyEngineIdentity(header, { method: 'POST', path: 'api/risk' }, secret, now + 70_000), null, 'expired after 60s (+5s skew)');
    const [p, sig] = header.split('.');
    const forged = Buffer.from(JSON.stringify({ ...JSON.parse(Buffer.from(p, 'base64url')), wrole: 'owner' })).toString('base64url');
    assert.equal(engineId.verifyEngineIdentity(`${forged}.${sig}`, { method: 'POST', path: 'api/risk' }, secret, now), null, 'tampered role');
  } finally {
    if (prev === undefined) delete process.env.ENGINE_SHARED_SECRET;
    else process.env.ENGINE_SHARED_SECRET = prev;
  }
});

// ── M19 task 4: authorization ──

/** Sign a user in and return what route handlers see (session + user + active workspace). */
const authFor = async (user) => sessions.lookupSession((await newSession(user)).token);
const addMember = (wsId, userId, role) => sql.query('INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)', [wsId, userId, role]);
const authzStatus = async (promise) => {
  try {
    await promise;
    return 200;
  } catch (err) {
    if (err.name !== 'AuthzError') throw err;
    return err.status;
  }
};

test('requireRole: viewer < editor < owner; non-members get 404; signed-out gets 401', async () => {
  const owner = await store.createUser('az-owner@x.com', 'h', 'O');
  const editor = await store.createUser('az-editor@x.com', 'h', 'E');
  const viewer = await store.createUser('az-viewer@x.com', 'h', 'V');
  const outsider = await store.createUser('az-out@x.com', 'h', 'X');
  const team = await workspaces.createWorkspace(owner.id, 'AuthZ Desk');
  await addMember(team.id, editor.id, 'editor');
  await addMember(team.id, viewer.id, 'viewer');

  const expected = {
    owner: { viewer: 200, editor: 200, owner: 200 },
    editor: { viewer: 200, editor: 200, owner: 403 },
    viewer: { viewer: 200, editor: 403, owner: 403 },
    outsider: { viewer: 404, editor: 404, owner: 404 },
  };
  const users = { owner, editor, viewer, outsider };
  for (const [who, row] of Object.entries(expected)) {
    const auth = await authFor(users[who]);
    for (const [min, status] of Object.entries(row)) {
      assert.equal(await authzStatus(authz.requireRole(auth, team.id, min)), status, `${who} needs ${min}`);
    }
  }
  assert.equal(await authzStatus(authz.requireRole(null, team.id, 'viewer')), 401);
  assert.equal(await authzStatus(authz.requireRole(await authFor(owner), crypto.randomUUID(), 'viewer')), 404, 'unknown workspace looks the same as someone else\'s');
  assert.equal(await authzStatus(authz.requireRole(await authFor(owner), 'garbage', 'viewer')), 404);

  const granted = await authz.requireRole(await authFor(viewer), team.id, 'viewer');
  assert.equal(granted.role, 'viewer', 'returns the workspace with the caller\'s role');
  assert.equal(authz.hasRole('owner', 'editor'), true);
  assert.equal(authz.hasRole('viewer', 'editor'), false);
});

test('one user can\'t read or change another user\'s workspace', async () => {
  const alice = await store.createUser('az-alice@x.com', 'h', 'Alice');
  const bob = await store.createUser('az-bob@x.com', 'h', 'Bob');
  const aliceAuth = await authFor(alice);
  const bobAuth = await authFor(bob);
  const bobTeam = await workspaces.createWorkspace(bob.id, 'Bob Research');

  for (const target of [bobAuth.workspace.id, bobTeam.id]) {
    for (const min of ['viewer', 'editor', 'owner']) {
      assert.equal(await authzStatus(authz.requireRole(aliceAuth, target, min)), 404, `alice → bob's ${target === bobTeam.id ? 'team' : 'personal'} (${min})`);
    }
    assert.equal(await workspaces.setActiveWorkspace(aliceAuth.session.id, alice.id, target), null, 'can\'t make it active either');
  }
  // The active workspace in the session can't be forged: a session pointed at bob's workspace by
  // hand still resolves to alice's own, because membership is re-checked on every request.
  await sql.query('UPDATE sessions SET workspace_id = $1 WHERE id = $2', [bobTeam.id, aliceAuth.session.id]);
  const token = (await newSession(alice)).token;
  await sql.query('UPDATE sessions SET workspace_id = $1 WHERE user_id = $2', [bobTeam.id, alice.id]);
  const resolved = await sessions.lookupSession(token);
  assert.equal(resolved.workspace.id, aliceAuth.workspace.id, 'falls back to alice\'s personal workspace');
  assert.ok(resolved.workspace.personal);
});

test('workspace settings: viewers list members, owners rename and delete, personal can\'t be deleted', async () => {
  const owner = await store.createUser('az-set-owner@x.com', 'h', 'Owner');
  const viewer = await store.createUser('az-set-viewer@x.com', 'h', 'Viewer');
  const team = await workspaces.createWorkspace(owner.id, 'Settings Desk');
  await addMember(team.id, viewer.id, 'viewer');
  const members = await workspaces.listMembers(team.id);
  assert.deepEqual(members.map((m) => [m.email, m.role]), [['az-set-owner@x.com', 'owner'], ['az-set-viewer@x.com', 'viewer']]);
  assert.ok(members.every((m) => !('passwordHash' in m)));

  const viewerAuth = await authFor(viewer);
  await workspaces.setActiveWorkspace(viewerAuth.session.id, viewer.id, team.id);
  assert.equal(await authzStatus(authz.requireRole(viewerAuth, team.id, 'owner')), 403, 'viewer can\'t rename or delete');

  assert.equal(await workspaces.renameWorkspace(team.id, 'Renamed Desk'), true);
  assert.equal((await workspaces.getMembership(owner.id, team.id)).name, 'Renamed Desk');

  const ownerAuth = await authFor(owner);
  await assert.rejects(workspaces.deleteWorkspace(ownerAuth.workspace.id), (e) => e.name === 'PersonalWorkspaceError');
  assert.equal(await workspaces.deleteWorkspace(team.id), true);
  assert.equal(await workspaces.deleteWorkspace(team.id), false);
  const after = await sessions.lookupSession((await newSession(viewer, { workspaceId: null })).token);
  assert.ok(after.workspace.personal, 'members of a deleted workspace land in their personal one');
});

test('engine routes: computations need editor, reads need viewer, nothing is unlisted', () => {
  const routes = engineRoutes.ENGINE_ROUTES;
  for (const [path, r] of Object.entries(routes)) {
    assert.ok(['viewer', 'editor', 'owner'].includes(r.minRole), `${path} has a minimum role`);
    const methods = Array.isArray(r.method) ? r.method : [r.method];
    for (const method of methods) {
      const role = engineRoutes.minimumRole(r, method);
      if (method !== 'GET') assert.ok(authz.hasRole(role, 'editor'), `${path}: ${method} changes data, so it needs editor`);
    }
  }
  for (const p of ['api/risk', 'api/stress', 'api/benchmark', 'api/alpaca/account']) assert.equal(routes[p].minRole, 'editor', p);
  for (const p of ['api/sample-portfolio', 'api/telemetry/cpu']) assert.equal(routes[p].minRole, 'viewer', p);
  assert.equal(Object.entries(routes).filter(([, r]) => r.public).map(([p]) => p).join(), 'health', 'only health is public');
  assert.ok(!Object.prototype.hasOwnProperty.call(routes, 'constructor'), 'no prototype keys');
  assert.equal(engineRoutes.routePolicyKey('api/v1/jobs/0192f3a4-0000-7000-8000-000000000002'), 'api/v1/jobs/{id}');
  assert.equal(engineRoutes.routePolicyKey('api/v1/jobs/0192f3a4-0000-7000-8000-000000000002/cancel'), 'api/v1/jobs/{id}/cancel');
  assert.equal(engineRoutes.minimumRole(routes['api/v1/jobs'], 'GET'), 'viewer');
  assert.equal(engineRoutes.minimumRole(routes['api/v1/jobs'], 'POST'), 'editor');
  const bt = '0192f3a4-0000-7000-8000-000000000002';
  assert.equal(engineRoutes.routePolicyKey(`api/v1/backtests/${bt}`), 'api/v1/backtests/{id}');
  assert.equal(engineRoutes.routePolicyKey(`api/v1/backtests/${bt}/cancel`), 'api/v1/backtests/{id}/cancel');
  assert.equal(engineRoutes.routePolicyKey(`api/v1/backtests/${bt}/series`), 'api/v1/backtests/{id}/series');
  assert.equal(engineRoutes.routePolicyKey(`api/v1/backtests/${bt}/other`), `api/v1/backtests/${bt}/other`, 'unknown sub-routes stay unlisted');
  assert.equal(engineRoutes.routePolicyKey('api/v1/backtests/not-a-uuid'), 'api/v1/backtests/not-a-uuid');
  assert.equal(engineRoutes.minimumRole(routes['api/v1/backtests'], 'POST'), 'editor', 'running a backtest needs editor');
  for (const p of ['api/v1/backtests/{id}', 'api/v1/backtests/{id}/series', 'api/v1/strategies/builtin']) {
    assert.equal(engineRoutes.minimumRole(routes[p], 'GET'), 'viewer', p);
  }
  assert.equal(engineRoutes.minimumRole(routes['api/v1/backtests/{id}/cancel'], 'POST'), 'editor');
  assert.equal(engineRoutes.minimumRole(routes['api/v1/backtests'], 'GET'), 'viewer', 'listing backtests is a read');
  assert.equal(engineRoutes.routePolicyKey(`api/v1/backtests/${bt}/artifacts/orders_csv`), 'api/v1/backtests/{id}/artifacts/{kind}');
  assert.equal(engineRoutes.routePolicyKey(`api/v1/backtests/${bt}/artifacts/strategies_csv`), 'api/v1/backtests/{id}/artifacts/{kind}');
  assert.equal(engineRoutes.routePolicyKey(`api/v1/backtests/${bt}/artifacts/secrets`), `api/v1/backtests/${bt}/artifacts/secrets`, 'unknown file kinds stay unlisted');
  assert.equal(engineRoutes.routePolicyKey(`api/v1/backtests/${bt}/artifacts/orders_csv/x`), `api/v1/backtests/${bt}/artifacts/orders_csv/x`);
  for (const p of ['api/v1/backtests/{id}/artifacts/{kind}', 'api/v1/datasets', 'api/v1/calendars']) {
    assert.equal(engineRoutes.minimumRole(routes[p], 'GET'), 'viewer', p);
    assert.ok(!engineRoutes.allowsMethod(routes[p], 'POST'), `${p} is read-only`);
  }
  // Data Studio (M1): previewing a file is part of uploading one, so viewers can't.
  assert.equal(engineRoutes.minimumRole(routes['api/v1/datasets/preview'], 'POST'), 'editor');
  assert.ok(!engineRoutes.allowsMethod(routes['api/v1/datasets/preview'], 'GET'), 'preview is POST only');
  assert.equal(engineRoutes.routePolicyKey('api/v1/datasets/preview'), 'api/v1/datasets/preview');
  assert.equal(engineRoutes.minimumRole(routes['api/v1/datasets/ingest'], 'POST'), 'editor', 'ingesting needs editor');
  assert.equal(engineRoutes.ENGINE_UPLOAD_ROUTE.minRole, 'editor', 'uploading needs editor');
  assert.ok(!Object.prototype.hasOwnProperty.call(routes, 'api/v1/uploads'), 'uploads never go through the 2 MB JSON proxy');
  // Browsing datasets is a read; renaming and deleting need editor.
  const ds = '0192f3a4-0000-7000-8000-000000000009';
  assert.equal(engineRoutes.routePolicyKey(`api/v1/datasets/${ds}`), 'api/v1/datasets/{id}');
  assert.equal(engineRoutes.routePolicyKey(`api/v1/datasets/versions/${ds}`), 'api/v1/datasets/versions/{id}');
  assert.equal(engineRoutes.routePolicyKey(`api/v1/datasets/versions/${ds}/bars`), 'api/v1/datasets/versions/{id}/bars');
  assert.equal(engineRoutes.routePolicyKey(`api/v1/datasets/versions/${ds}/runs`), 'api/v1/datasets/versions/{id}/runs');
  assert.equal(engineRoutes.routePolicyKey(`api/v1/datasets/versions/${ds}/other`), `api/v1/datasets/versions/${ds}/other`, 'unknown sub-routes stay unlisted');
  assert.equal(engineRoutes.routePolicyKey('api/v1/datasets/not-a-uuid'), 'api/v1/datasets/not-a-uuid');
  for (const p of ['api/v1/datasets/{id}', 'api/v1/datasets/versions/{id}', 'api/v1/datasets/versions/{id}/runs', 'api/v1/datasets/versions/{id}/bars']) {
    assert.equal(engineRoutes.minimumRole(routes[p], 'GET'), 'viewer', p);
  }
  assert.equal(engineRoutes.minimumRole(routes['api/v1/datasets/{id}'], 'PATCH'), 'editor', 'renaming needs editor');
  assert.equal(engineRoutes.minimumRole(routes['api/v1/datasets/versions/{id}'], 'DELETE'), 'editor', 'deleting needs editor');
  assert.ok(!engineRoutes.allowsMethod(routes['api/v1/datasets/{id}'], 'DELETE'), 'datasets are deleted version by version');
  assert.ok(!engineRoutes.allowsMethod(routes['api/v1/datasets/versions/{id}/bars'], 'POST'));
});

test('platform admin: seeded once from ADMIN_EMAIL, then only from the database', async () => {
  // admin@x.com signed up earlier (2FA test) and became the first admin.
  assert.equal((await store.findUser('admin@x.com')).role, 'admin');
  const prev = process.env.ADMIN_EMAIL;
  try {
    process.env.ADMIN_EMAIL = 'late-seed@x.com';
    assert.equal((await store.createUser('late-seed@x.com', 'h', 'Late')).role, 'user', 'an admin exists, so the seed email is just a user');
    delete process.env.ADMIN_EMAIL;
    assert.equal((await store.createUser('no-seed@x.com', 'h', 'N')).role, 'user');
  } finally {
    process.env.ADMIN_EMAIL = prev;
  }
});

test('platform admin: grant/revoke rotates sessions, turns on 2FA, keeps at least one admin', async () => {
  const u = await store.createUser('promote@x.com', 'h', 'Promote', false);
  const { token } = await newSession(u);
  const granted = await store.setPlatformRole('promote@x.com', 'admin');
  assert.equal(granted.role, 'admin');
  assert.equal(granted.twoFactor, true, 'admins always have two-step login');
  assert.equal(await sessions.lookupSession(token), null, 'privilege change ends existing sessions (new token on next sign-in)');
  const fresh = await sessions.lookupSession((await newSession(granted)).token);
  assert.equal(fresh.user.role, 'admin', 'role comes from the database');

  assert.equal((await store.setPlatformRole('promote@x.com', 'user')).role, 'user');
  assert.equal(await store.setPlatformRole('nobody@x.com', 'admin'), undefined);
  await store.setPlatformRole('old.admin@x.com', 'user'); // imported admin from the import test
  const admins = (await sql.query(`SELECT email FROM users WHERE platform_role = 'admin'`)).rows.map((r) => r.email);
  assert.deepEqual(admins, ['admin@x.com']);
  await assert.rejects(store.setPlatformRole('admin@x.com', 'user'), (e) => e.name === 'LastAdminError');
  assert.equal((await store.findUser('admin@x.com')).role, 'admin', 'still admin');
});

test('admin CLI: list, grant, revoke', async () => {
  const { execFileSync } = await import('node:child_process');
  const run = (...args) =>
    execFileSync(process.execPath, [path.join(root, 'scripts/admin.mjs'), ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, DATABASE_URL: testUrl } });
  assert.match(run('list'), /admin@x\.com/);
  await store.createUser('cli@x.com', 'h', 'Cli', false);
  assert.match(run('grant', 'CLI@x.com'), /now an admin/);
  assert.equal((await store.findUser('cli@x.com')).role, 'admin');
  assert.equal((await store.findUser('cli@x.com')).twoFactor, true);
  assert.match(run('revoke', 'cli@x.com'), /regular user/);
  assert.throws(() => run('revoke', 'admin@x.com'), /only admin/);
  assert.throws(() => run('grant', 'ghost@x.com'), /No account/);
});

// ── M19 task 8: encrypted broker keys ──

const ALPACA = { keyId: 'PKTESTKEY0000000WXYZ', secretKey: 'sEcReTkEy0123456789abcdefghijklmnopqrstuv' };
const allRowText = async (sqlText, params) => JSON.stringify((await sql.query(sqlText, params)).rows, (k, v) => (v && v.type === 'Buffer' ? Buffer.from(v.data).toString('latin1') : v));

test('vault: keys are encrypted at rest; only metadata comes back', async () => {
  const owner = await store.createUser('vault-owner@x.com', 'h', 'V');
  const ws = (await workspaces.listWorkspaces(owner.id))[0];
  const info = await vault.putSecret(ws.id, 'alpaca_paper', { ...ALPACA, extra: 'ignored' }, owner.id);
  assert.deepEqual(Object.keys(info).sort(), ['createdAt', 'createdBy', 'hint', 'kind', 'label', 'lastUsedAt', 'updatedAt']);
  assert.equal(info.hint, 'WXYZ', 'last 4 characters of the key id only');
  assert.equal(info.createdBy, 'vault-owner@x.com');

  const raw = (await allRowText('SELECT * FROM secrets WHERE workspace_id = $1', [ws.id])) + (await allRowText('SELECT * FROM workspace_keys WHERE workspace_id = $1', [ws.id]));
  for (const plain of [ALPACA.secretKey, ALPACA.keyId, ALPACA.secretKey.slice(0, 12)]) assert.ok(!raw.includes(plain), 'no plaintext in the database');
  assert.ok(!raw.includes(Buffer.from(MASTER_V1, 'base64').toString('latin1')), 'master key never stored');

  const listed = await vault.listSecrets(ws.id);
  assert.equal(listed.length, 1);
  assert.ok(!JSON.stringify(listed).includes(ALPACA.secretKey) && !JSON.stringify(listed).includes(ALPACA.keyId));

  let seen;
  const out = await vault.useSecret(ws.id, 'alpaca_paper', async (v) => {
    seen = { ...v };
    return 'broker-call-result';
  });
  assert.equal(out, 'broker-call-result');
  assert.deepEqual(seen, ALPACA, 'the worker gets exactly what was stored (extra fields dropped)');
  assert.ok((await vault.listSecrets(ws.id))[0].lastUsedAt, 'use is recorded');
});

test('vault: replace, delete, validation, unknown kinds', async () => {
  const owner = await store.findUser('vault-owner@x.com');
  const ws = (await workspaces.listWorkspaces(owner.id))[0];
  await vault.putSecret(ws.id, 'alpaca_paper', { keyId: 'PKNEWKEY0000000ABCD', secretKey: 'another-secret-key-0123456789' }, owner.id);
  const [replaced] = await vault.listSecrets(ws.id);
  assert.equal(replaced.hint, 'ABCD');
  assert.equal(replaced.lastUsedAt, null, 'a replaced key starts unused');
  assert.equal(await vault.useSecret(ws.id, 'alpaca_paper', async (v) => v.secretKey), 'another-secret-key-0123456789');
  assert.equal((await sql.query('SELECT count(*)::int AS n FROM secrets WHERE workspace_id = $1', [ws.id])).rows[0].n, 1, 'one row per kind');

  for (const bad of [null, {}, { keyId: 'PK', secretKey: ALPACA.secretKey }, { keyId: ALPACA.keyId, secretKey: 'has spaces in it 123' }, { keyId: ALPACA.keyId }]) {
    await assert.rejects(vault.putSecret(ws.id, 'alpaca_paper', bad, owner.id), (e) => e.name === 'SecretValidationError' && !e.message.includes(ALPACA.secretKey));
  }
  assert.equal(vault.isSecretKind('alpaca_paper'), true);
  assert.equal(vault.isSecretKind('constructor'), false);
  assert.equal(vault.isSecretKind('alpaca_live'), false);

  assert.equal(await vault.deleteSecret(ws.id, 'alpaca_paper'), true);
  assert.equal(await vault.deleteSecret(ws.id, 'alpaca_paper'), false);
  await assert.rejects(vault.useSecret(ws.id, 'alpaca_paper', async () => 1), /no alpaca_paper secret/);
});

test('vault: ciphertext is bound to its workspace; tampering is detected', async () => {
  const a = await store.createUser('vault-a@x.com', 'h', 'A');
  const b = await store.createUser('vault-b@x.com', 'h', 'B');
  const wsA = (await workspaces.listWorkspaces(a.id))[0];
  const wsB = (await workspaces.listWorkspaces(b.id))[0];
  await vault.putSecret(wsA.id, 'alpaca_paper', ALPACA, a.id);
  await vault.putSecret(wsB.id, 'alpaca_paper', { keyId: 'PKBBBBBBBBBBBBBBBBBB', secretKey: 'b-secret-0123456789abcdef' }, b.id);
  const keys = (await sql.query('SELECT workspace_id, wrapped_key FROM workspace_keys WHERE workspace_id = ANY($1)', [[wsA.id, wsB.id]])).rows;
  assert.notDeepEqual(keys[0].wrapped_key, keys[1].wrapped_key, 'each workspace has its own data key');

  // Someone with database access copies A's secret into B's row: it must not decrypt.
  await sql.query(`UPDATE secrets SET ciphertext = (SELECT ciphertext FROM secrets WHERE workspace_id = $1) WHERE workspace_id = $2`, [wsA.id, wsB.id]);
  await assert.rejects(vault.useSecret(wsB.id, 'alpaca_paper', async () => 1), /unable to authenticate|Unsupported state/);
  // ...or flips one byte of A's ciphertext.
  await sql.query(`UPDATE secrets SET ciphertext = set_byte(ciphertext, 40, get_byte(ciphertext, 40) # 1) WHERE workspace_id = $1`, [wsA.id]);
  await assert.rejects(vault.useSecret(wsA.id, 'alpaca_paper', async () => 1), /unable to authenticate|Unsupported state/);

  // Deleting a workspace takes its keys and secrets with it.
  const team = await workspaces.createWorkspace(a.id, 'Vault Team');
  await vault.putSecret(team.id, 'alpaca_paper', ALPACA, a.id);
  await workspaces.deleteWorkspace(team.id);
  assert.equal((await sql.query('SELECT count(*)::int AS n FROM secrets WHERE workspace_id = $1', [team.id])).rows[0].n, 0);
  assert.equal((await sql.query('SELECT count(*)::int AS n FROM workspace_keys WHERE workspace_id = $1', [team.id])).rows[0].n, 0);
});

test('vault: master key parsing and configuration errors', () => {
  const k = () => crypto.randomBytes(32).toString('base64');
  assert.equal(vault.parseKeyring(k()).current.version, 1, 'plain key = version 1');
  const ring = vault.parseKeyring(`3:${k()},1:${k()}`);
  assert.equal(ring.current.version, 3);
  assert.deepEqual([...ring.byVersion.keys()], [3, 1]);
  assert.equal(vault.parseKeyring(crypto.randomBytes(32).toString('base64url')).current.key.length, 32, 'base64url accepted');
  for (const bad of [undefined, '', 'tooshort', crypto.randomBytes(16).toString('base64'), `1:${k()},1:${k()}`, `${k()},${k()}`]) {
    assert.throws(() => vault.parseKeyring(bad), (e) => e.name === 'VaultConfigError', String(bad).slice(0, 12));
  }
});

test('vault: master-key rotation re-wraps data keys; secrets survive; old key can then be dropped', async () => {
  const owner = await store.createUser('vault-rot@x.com', 'h', 'R');
  const ws = (await workspaces.listWorkspaces(owner.id))[0];
  await vault.putSecret(ws.id, 'alpaca_paper', ALPACA, owner.id);
  const MASTER_V2 = crypto.randomBytes(32).toString('base64');
  try {
    process.env.SECRETS_MASTER_KEY = `2:${MASTER_V2}`;
    await assert.rejects(vault.useSecret(ws.id, 'alpaca_paper', async () => 1), /version 1, which is not in SECRETS_MASTER_KEY/, 'dropping the old key too early is a clear error');
    await assert.rejects(vault.rotateMasterKey(), (e) => e.name === 'VaultConfigError');

    process.env.SECRETS_MASTER_KEY = `2:${MASTER_V2},1:${MASTER_V1}`;
    assert.ok((await vault.rotateMasterKey()) >= 1);
    assert.equal(await vault.rotateMasterKey(), 0, 'idempotent');
    assert.equal((await sql.query('SELECT count(*)::int AS n FROM workspace_keys WHERE master_version <> 2')).rows[0].n, 0);

    process.env.SECRETS_MASTER_KEY = `2:${MASTER_V2}`;
    assert.equal(await vault.useSecret(ws.id, 'alpaca_paper', async (v) => v.secretKey), ALPACA.secretKey, 'readable with only the new key');
    const fresh = await workspaces.createWorkspace(owner.id, 'After rotation');
    await vault.putSecret(fresh.id, 'alpaca_paper', ALPACA, owner.id);
    assert.equal((await sql.query('SELECT master_version FROM workspace_keys WHERE workspace_id = $1', [fresh.id])).rows[0].master_version, 2, 'new keys use the current version');

    // The command-line rotation uses the same code.
    const { execFileSync } = await import('node:child_process');
    const MASTER_V3 = crypto.randomBytes(32).toString('base64');
    const out = execFileSync(process.execPath, [path.join(root, 'scripts/secrets-rotate.mjs')], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      env: { ...process.env, DATABASE_URL: testUrl, SECRETS_MASTER_KEY: `3:${MASTER_V3},2:${MASTER_V2}` },
    });
    assert.match(out, /0 left on older versions/);
    process.env.SECRETS_MASTER_KEY = `3:${MASTER_V3}`;
    assert.equal(await vault.useSecret(fresh.id, 'alpaca_paper', async (v) => v.keyId), ALPACA.keyId);
  } finally {
    process.env.SECRETS_MASTER_KEY = MASTER_V1;
  }
});

test('vault: no page, component or route can decrypt (useSecret is worker-only)', () => {
  const offenders = [];
  const walk = (dir) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (/\.(tsx?|jsx?|mjs)$/.test(e.name) && /\buseSecret\b|\bopen\s*\(\s*key/.test(fs.readFileSync(p, 'utf8'))) offenders.push(path.relative(root, p));
    }
  };
  for (const dir of ['app', 'components']) walk(path.join(root, dir));
  assert.deepEqual(offenders, []);
});

test('setup script: strong AUTH_SECRET, default DATABASE_URL, never overwrites good values', async () => {
  const { execFileSync } = await import('node:child_process');
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'spectra-setup-test-'));
  fs.mkdirSync(path.join(dir, 'scripts'));
  fs.copyFileSync(path.join(root, 'scripts/setup-env.mjs'), path.join(dir, 'scripts/setup-env.mjs'));
  fs.copyFileSync(path.join(root, '.env.example'), path.join(dir, '.env.example'));
  const env = path.join(dir, '.env.local');
  const run = () => execFileSync(process.execPath, [path.join(dir, 'scripts/setup-env.mjs')], { encoding: 'utf8' });
  const valueOf = (key) => fs.readFileSync(env, 'utf8').match(new RegExp(`^${key}=(.*)$`, 'm'))?.[1];
  try {
    const out = run();
    const first = valueOf('AUTH_SECRET');
    assert.ok(first.length >= 60, 'long random secret');
    assert.ok(!out.includes(first), 'secret is never printed');
    assert.match(valueOf('DATABASE_URL'), /^postgres(ql)?:\/\//, 'a default DATABASE_URL is filled in');
    assert.ok(valueOf('ENGINE_SHARED_SECRET').length >= 60, 'engine secret generated');
    assert.notEqual(valueOf('ENGINE_SHARED_SECRET'), first, 'distinct from AUTH_SECRET');
    const master = valueOf('SECRETS_MASTER_KEY');
    assert.equal(Buffer.from(master, 'base64').length, 32, 'master key: 32 random bytes');
    run();
    assert.equal(valueOf('AUTH_SECRET'), first, 'a good secret is left alone');
    fs.writeFileSync(env, fs.readFileSync(env, 'utf8').replace(/^DATABASE_URL=.*$/m, 'DATABASE_URL=postgres://me@db.example/prod'));
    fs.writeFileSync(env, fs.readFileSync(env, 'utf8').replace(/^AUTH_SECRET=.*$/m, 'AUTH_SECRET=spectralabs_jwt_default_secret_key_32bytes_required'));
    run();
    assert.notEqual(valueOf('AUTH_SECRET'), 'spectralabs_jwt_default_secret_key_32bytes_required', 'the retired default is replaced');
    assert.equal(valueOf('DATABASE_URL'), 'postgres://me@db.example/prod', 'a custom DATABASE_URL is kept');
    assert.equal(valueOf('SECRETS_MASTER_KEY'), master, 'the master key is never replaced');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

// ── M19 task 7: rate limits and lockouts that survive restarts ──

test('rate limiter: counts in Postgres, refuses over the limit, window resets', async () => {
  for (let i = 0; i < 3; i++) assert.ok((await rl.hit('t', 3, 60_000)).ok);
  const r = await rl.hit('t', 3, 60_000);
  assert.ok(!r.ok && r.retryAfter > 0 && r.retryAfter <= 60);
  assert.ok(!(await rl.peek('t', 3)).ok);
  await rl.reset('t');
  assert.ok((await rl.peek('t', 3)).ok);
  assert.ok((await rl.hit('t', 3, 60_000)).ok, 'starts over after reset');

  for (let i = 0; i < 4; i++) await rl.hit('w', 3, 60_000);
  assert.ok(!(await rl.hit('w', 3, 60_000)).ok);
  await sql.query(`UPDATE rate_limits SET reset_at = now() - interval '1 second' WHERE key = 'w'`);
  assert.ok((await rl.peek('w', 3)).ok, 'an ended window no longer counts');
  assert.ok((await rl.hit('w', 3, 60_000)).ok, 'the next hit opens a new window');
  assert.equal((await sql.query(`SELECT count FROM rate_limits WHERE key = 'w'`)).rows[0].count, 1);
});

test('rate limiter: atomic under parallel requests (exactly max get through)', async () => {
  const results = await Promise.all(Array.from({ length: 25 }, () => rl.hit('burst', 5, 60_000)));
  assert.equal(results.filter((r) => r.ok).length, 5);
});

test('rate limiter: counters survive a restart', async () => {
  for (let i = 0; i < 2; i++) await rl.hit('persist', 2, 60_000);
  await restart();
  assert.ok(!(await rl.hit('persist', 2, 60_000)).ok, 'still over the limit after the restart');
});

test('rate limiter: hitAll stops at the first exceeded limit and names it', async () => {
  const limits = [
    ['all:a', 5, 60_000, 'first thing'],
    ['all:b', 1, 60_000, 'second thing'],
  ];
  assert.deepEqual(await rl.hitAll(limits), { ok: true });
  const r = await rl.hitAll(limits);
  assert.equal(r.ok, false);
  assert.equal(r.what, 'second thing');
});

test('lockout: 10 wrong passwords lock the account for 15 minutes; one email per lock', async () => {
  const u = await store.createUser('locked@x.com', 'h', 'L');
  assert.equal(await lockout.lockedUntil(u.id), null);
  let last;
  for (let i = 1; i <= 9; i++) {
    last = await lockout.recordFailedLogin(u.id);
    assert.equal(last.lockedUntil, null, `not locked after ${i}`);
    assert.equal(last.justLocked, false);
  }
  last = await lockout.recordFailedLogin(u.id);
  assert.equal(last.failures, 10);
  assert.equal(last.justLocked, true, 'the 10th failure locks, and says so once');
  const until = await lockout.lockedUntil(u.id);
  assert.ok(until && Math.abs(until - Date.now() - 15 * 60_000) < 10_000, 'locked for ~15 minutes');
  assert.match(lockout.lockMessage(until), /locked for 15 more minutes/);

  await restart();
  assert.ok(await lockout.lockedUntil(u.id), 'the lock survives a restart');

  assert.equal(await lockout.shouldSendLockoutEmail(u.id), true, 'first lockout email goes out');
  assert.equal(await lockout.shouldSendLockoutEmail(u.id), false, 'no second email within 6 hours');
});

test('lockout: parallel wrong guesses can\'t slip past the threshold', async () => {
  const u = await store.createUser('parallel-lock@x.com', 'h', 'P');
  const results = await Promise.all(Array.from({ length: 15 }, () => lockout.recordFailedLogin(u.id)));
  assert.equal(results.filter((r) => r.justLocked).length, 1, 'exactly one request starts the lock');
  assert.ok(await lockout.lockedUntil(u.id));
});

test('lockout: expires, starts a fresh count, and is cleared by success or reset', async () => {
  const u = await store.findUser('locked@x.com');
  await sql.query(`UPDATE users SET locked_until = now() - interval '1 second' WHERE id = $1`, [u.id]);
  assert.equal(await lockout.lockedUntil(u.id), null, 'lock ends after 15 minutes');
  const after = await lockout.recordFailedLogin(u.id);
  assert.equal(after.failures, 1, 'a new lock period starts counting from scratch');
  assert.equal(after.lockedUntil, null);

  for (let i = 0; i < 4; i++) await lockout.recordFailedLogin(u.id);
  await sql.query(`UPDATE users SET last_failed_login_at = now() - interval '2 hours' WHERE id = $1`, [u.id]);
  assert.equal((await lockout.recordFailedLogin(u.id)).failures, 1, 'failures older than an hour are forgotten');

  for (let i = 0; i < 10; i++) await lockout.recordFailedLogin(u.id);
  assert.ok(await lockout.lockedUntil(u.id));
  await lockout.clearFailedLogins(u.id); // successful sign-in or password reset
  assert.equal(await lockout.lockedUntil(u.id), null);
  assert.equal((await sql.query('SELECT failed_login_count FROM users WHERE id = $1', [u.id])).rows[0].failed_login_count, 0);
  assert.equal(await lockout.lockedUntil(crypto.randomUUID()), null, 'unknown user: no lock, no error');
  assert.deepEqual(await lockout.recordFailedLogin(crypto.randomUUID()), { failures: 0, lockedUntil: null, justLocked: false });
});

test('Argon2id: hash format, verification, and non-matching params trigger needsRehash', async () => {
  const argon2 = require('@node-rs/argon2');
  const password = 'Secur3P@ssword!';
  const hash = await store.hashPassword(password);
  assert.match(hash, /^\$argon2id\$v=19\$m=19456,t=2,p=1\$/, 'matches OWASP Argon2id params');

  const correct = await store.verifyPassword(password, hash);
  assert.deepEqual(correct, { ok: true, needsRehash: false });

  const wrong = await store.verifyPassword('WrongP@ssword1', hash);
  assert.deepEqual(wrong, { ok: false, needsRehash: false });

  // Hash generated with different cost parameters should verify ok but request rehash
  const olderHash = await argon2.hash(password, {
    memoryCost: 4096,
    timeCost: 1,
    parallelism: 1,
    algorithm: argon2.Algorithm.Argon2id,
  });
  const olderCheck = await store.verifyPassword(password, olderHash);
  assert.deepEqual(olderCheck, { ok: true, needsRehash: true });
});

test('legacy PBKDF2: verifies correctly and requests rehash', async () => {
  const password = 'LegacyP@ssword1!';
  const salt = crypto.randomBytes(16).toString('hex');
  const pbkdf2Hash = crypto.pbkdf2Sync(password, salt, 100_000, 64, 'sha512').toString('hex');
  const legacyStored = `${salt}:${pbkdf2Hash}`;

  const valid = await store.verifyPassword(password, legacyStored);
  assert.deepEqual(valid, { ok: true, needsRehash: true });

  const invalid = await store.verifyPassword('WrongP@ssword1', legacyStored);
  assert.deepEqual(invalid, { ok: false, needsRehash: false });
});

test('password verification fails closed and never throws on malformed hashes', async () => {
  for (const bad of ['', 'random-string', '$argon2id$', '$argon2id$v=19$m=invalid', 'badhex:1234', '1234:badhex', null, undefined]) {
    const res = await store.verifyPassword('Passw0rd!', bad);
    assert.deepEqual(res, { ok: false, needsRehash: false }, `failed closed for ${bad}`);
  }
});

test('DoS guard: passwords longer than 256 chars are rejected', async () => {
  const huge = 'A'.repeat(257);
  await assert.rejects(async () => store.hashPassword(huge), /256/);
  const verifyRes = await store.verifyPassword(huge, '$argon2id$v=19$m=19456,t=2,p=1$dummy');
  assert.deepEqual(verifyRes, { ok: false, needsRehash: false });
});

test('rehashPassword: compare-and-set, preserves sessions and password_changed_at', async () => {
  const email = 'rehash-test@x.com';
  const salt = crypto.randomBytes(16).toString('hex');
  const legacyHash = `${salt}:${crypto.pbkdf2Sync('OldPassw0rd!', salt, 100_000, 64, 'sha512').toString('hex')}`;
  const u = await store.createUser(email, legacyHash, 'Rehash Tester');

  const beforeRow = (await sql.query('SELECT password_changed_at FROM users WHERE id = $1', [u.id])).rows[0];
  const { token } = await newSession(u);
  assert.ok(await sessions.lookupSession(token), 'session active');

  const newHash = await store.hashPassword('OldPassw0rd!');
  const ok = await store.rehashPassword(u.id, newHash, legacyHash);
  assert.equal(ok, true, 'rehash succeeded');

  const afterRow = (await sql.query('SELECT password_hash, password_changed_at FROM users WHERE id = $1', [u.id])).rows[0];
  assert.equal(afterRow.password_hash, newHash, 'hash was updated');
  assert.deepEqual(afterRow.password_changed_at, beforeRow.password_changed_at, 'password_changed_at was not touched');
  assert.ok(await sessions.lookupSession(token), 'session was NOT revoked on rehash');

  // Stale compare-and-set should fail
  const failed = await store.rehashPassword(u.id, 'another-hash', 'mismatched-old-hash');
  assert.equal(failed, false, 'stale CAS refused');
  const unchangedRow = (await sql.query('SELECT password_hash FROM users WHERE id = $1', [u.id])).rows[0];
  assert.equal(unchangedRow.password_hash, newHash);
});

test('isBreachedPassword: HaveIBeenPwned range API, k-anonymity and fail-open', async () => {
  const pass = 'P@ssword123!';
  const sha1 = crypto.createHash('sha1').update(pass).digest('hex').toUpperCase();
  const prefix = sha1.slice(0, 5);
  const suffix = sha1.slice(5);

  // 1. Breached hit (count > 0)
  const mockFetchHit = async (url, opts) => {
    assert.ok(url.endsWith(prefix));
    assert.equal(opts.headers?.['Add-Padding'], 'true');
    return {
      ok: true,
      status: 200,
      text: async () => `${suffix}:42\r\nOTHERHASH35CHARSOTHERHASH35CHARSOTH:10\r\n`,
    };
  };
  assert.equal(await breached.isBreachedPassword(pass, { fetchImpl: mockFetchHit }), true);

  // 2. Padding row (count === 0)
  const mockFetchPadding = async () => ({
    ok: true,
    status: 200,
    text: async () => `${suffix}:0\r\nOTHERHASH35CHARSOTHERHASH35CHARSOTH:10\r\n`,
  });
  assert.equal(await breached.isBreachedPassword(pass, { fetchImpl: mockFetchPadding }), false);

  // 3. Not in response
  const mockFetchNotFound = async () => ({
    ok: true,
    status: 200,
    text: async () => `OTHERHASH35CHARSOTHERHASH35CHARSOTH:10\r\n`,
  });
  assert.equal(await breached.isBreachedPassword(pass, { fetchImpl: mockFetchNotFound }), false);

  // 4. Fail open on 500
  const mockFetch500 = async () => ({
    ok: false,
    status: 500,
    text: async () => 'Internal Server Error',
  });
  assert.equal(await breached.isBreachedPassword(pass, { fetchImpl: mockFetch500 }), false);

  // 5. Fail open on network error / throw
  const mockFetchError = async () => {
    throw new Error('Connection refused');
  };
  assert.equal(await breached.isBreachedPassword(pass, { fetchImpl: mockFetchError }), false);

  // 6. HIBP_DISABLED skips check
  process.env.HIBP_DISABLED = '1';
  let called = false;
  const mockFetchNever = async () => {
    called = true;
    return { ok: true, status: 200, text: async () => `${suffix}:999\r\n` };
  };
  assert.equal(await breached.isBreachedPassword(pass, { fetchImpl: mockFetchNever }), false);
  assert.equal(called, false, 'fetch was skipped when HIBP_DISABLED=1');
  delete process.env.HIBP_DISABLED;
});

test('re-hash upgrade flow on login: legacy PBKDF2 hash transparently upgraded to Argon2id without session loss', async () => {
  const email = 'rehash-login@x.com';
  const password = 'UpgradeMe123!';
  const salt = crypto.randomBytes(16).toString('hex');
  const legacyHash = `${salt}:${crypto.pbkdf2Sync(password, salt, 100_000, 64, 'sha512').toString('hex')}`;
  const u = await store.createUser(email, legacyHash, 'Upgrade User');

  // Verify returns needsRehash: true
  const firstVerify = await store.verifyPassword(password, u.passwordHash);
  assert.equal(firstVerify.ok, true);
  assert.equal(firstVerify.needsRehash, true);

  // Simulate login route rehash
  if (firstVerify.needsRehash) {
    const newHash = await store.hashPassword(password);
    await store.rehashPassword(u.id, newHash, u.passwordHash);
  }

  // After rehash, user record has Argon2id hash and verify returns needsRehash: false
  const updated = await store.findUser(email);
  assert.ok(updated.passwordHash.startsWith('$argon2id$'));
  const secondVerify = await store.verifyPassword(password, updated.passwordHash);
  assert.equal(secondVerify.ok, true);
  assert.equal(secondVerify.needsRehash, false);
});

// ── M19 task 10: authenticator app, recovery codes, sign-in challenge, step-up ──

test('totp: RFC 6238 test vectors, base32, otpauth URI, ±1 step window', () => {
  const sec = Buffer.from('12345678901234567890');
  for (const [t, code] of [[59, '94287082'], [1111111109, '07081804'], [1111111111, '14050471'], [1234567890, '89005924'], [2000000000, '69279037'], [20000000000, '65353130']]) {
    assert.equal(totp.totpAt(sec, Math.floor(t / 30), 8), code, `T=${t}`);
  }
  assert.equal(totp.totpAt(sec, 1), '287082', '6-digit = low 6 digits');
  assert.equal(totp.base32Encode(Buffer.from('foobar')), 'MZXW6YTBOI');
  assert.equal(totp.base32Decode('mzxw 6ytb oi').toString(), 'foobar');
  assert.throws(() => totp.base32Decode('M1'), /invalid base32/);

  const uri = totp.otpauthUri('a@b.com', sec);
  assert.match(uri, /^otpauth:\/\/totp\/Spectra%20Labs%3Aa%40b\.com\?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Spectra%20Labs&algorithm=SHA1&digits=6&period=30$/);

  const now = 1_800_000_000_000;
  const step = totp.stepAt(now);
  for (const d of [-1, 0, 1]) assert.equal(totp.matchTotp(sec, totp.totpAt(sec, step + d), now), step + d, `drift ${d}`);
  for (const d of [-2, 2]) assert.equal(totp.matchTotp(sec, totp.totpAt(sec, step + d), now), null, `drift ${d} rejected`);
  assert.equal(totp.matchTotp(sec, 'abcdef', now), null);
  assert.equal(totp.matchTotp(sec, '12345', now), null);
});

const codeFor = (secretB32, offsetSteps = 0) => totp.totpAt(totp.base32Decode(secretB32), totp.stepAt(Date.now()) + offsetSteps);

test('mfa: enrolment is pending until a first code confirms it; the seed is encrypted', async () => {
  const u = await store.createUser('mfa-enrol@x.com', 'h', 'M', false);
  assert.deepEqual(await mfa.getMfaStatus(u.id), { totp: 'off', totpSince: null, recoveryCodesLeft: 0 });
  const { secret, uri } = await mfa.startTotpEnrollment(u);
  assert.match(secret, /^[A-Z2-7]{32}$/, '160-bit secret');
  assert.ok(uri.includes(`secret=${secret}`) && uri.includes('mfa-enrol%40x.com'));
  assert.equal((await mfa.getMfaStatus(u.id)).totp, 'pending');
  assert.equal(await mfa.hasTotp(u.id), false, 'pending doesn\'t count');
  const raw = await allRowText('SELECT * FROM user_totp WHERE user_id = $1', [u.id]);
  assert.ok(!raw.includes(secret) && !raw.includes(totp.base32Decode(secret).toString('latin1')), 'seed never stored in clear');

  await assert.rejects(mfa.confirmTotpEnrollment(u.id, '000000'), (e) => e.code === 'bad_code');
  const codes = await mfa.confirmTotpEnrollment(u.id, codeFor(secret));
  assert.equal(codes.length, 10);
  assert.equal(new Set(codes).size, 10);
  for (const c of codes) assert.match(c, /^[A-HJ-NP-Z2-9]{5}-[A-HJ-NP-Z2-9]{5}$/);
  const st = await mfa.getMfaStatus(u.id);
  assert.equal(st.totp, 'on');
  assert.equal(st.recoveryCodesLeft, 10);
  assert.equal((await store.findUser('mfa-enrol@x.com')).twoFactor, true, 'turning the app on turns two-step login on');
  const stored = await allRowText('SELECT * FROM recovery_codes WHERE user_id = $1', [u.id]);
  assert.ok(codes.every((c) => !stored.includes(c) && !stored.includes(c.replace('-', ''))), 'recovery codes stored hashed');

  await assert.rejects(mfa.startTotpEnrollment(u), (e) => e.code === 'already_on');
  await assert.rejects(mfa.confirmTotpEnrollment(u.id, codeFor(secret)), (e) => e.code === 'already_on');
  mfaFixture.user = u;
  mfaFixture.secret = secret;
  mfaFixture.codes = codes;
});
const mfaFixture = {};

test('mfa: each authenticator code works once (replay protection), also under parallel use', async () => {
  const { user, secret } = mfaFixture;
  // The confirm step used the current code: it can't be reused.
  assert.equal(await mfa.verifyTotpCode(user.id, codeFor(secret)), false, 'code used at enrolment is spent');
  const next = codeFor(secret, 1);
  const results = await Promise.all(Array.from({ length: 5 }, () => mfa.verifyTotpCode(user.id, next)));
  assert.equal(results.filter(Boolean).length, 1, 'exactly one request wins');
  assert.equal(await mfa.verifyTotpCode(user.id, codeFor(secret, -1)), false, 'an older step can\'t be used after a newer one');
  assert.equal(await mfa.verifyTotpCode(user.id, '123456'), false);
  const other = await store.createUser('mfa-none@x.com', 'h', 'N');
  assert.equal(await mfa.verifyTotpCode(other.id, next), false, 'no authenticator → no');
});

test('mfa: recovery codes work once, any case or spacing; regenerating replaces them', async () => {
  const { user, codes } = mfaFixture;
  const r = await mfa.consumeRecoveryCode(user.id, codes[0].toLowerCase().replace('-', ' '));
  assert.deepEqual(r, { ok: true, left: 9 });
  assert.equal((await mfa.consumeRecoveryCode(user.id, codes[0])).ok, false, 'single use');
  assert.equal((await mfa.consumeRecoveryCode(user.id, 'ZZZZZ-ZZZZZ')).ok, false);
  assert.equal((await mfa.consumeRecoveryCode(user.id, '123456')).ok, false, 'digits are never treated as a recovery code');

  const viaDispatch = await mfa.verifySecondFactor(user.id, codes[1]);
  assert.deepEqual(viaDispatch, { ok: true, method: 'recovery_code', recoveryCodesLeft: 8 });

  const fresh = await mfa.regenerateRecoveryCodes(user.id);
  assert.equal((await mfa.consumeRecoveryCode(user.id, codes[2])).ok, false, 'old codes stop working');
  assert.equal((await mfa.consumeRecoveryCode(user.id, fresh[0])).ok, true);
  assert.equal((await mfa.getMfaStatus(user.id)).recoveryCodesLeft, 9);
  mfaFixture.codes = fresh;
});

test('mfa: sign-in challenge is single-use, short-lived and attempt-limited', async () => {
  const email = 'mfa-enrol@x.com';
  const token = await mfa.createMfaChallenge(email);
  assert.equal(await mfa.checkMfaChallenge(email, token), 'ok');
  assert.equal(await mfa.checkMfaChallenge(email, 'x'.repeat(43)), 'missing', 'wrong token');
  assert.equal(await mfa.checkMfaChallenge('someone-else@x.com', token), 'missing', 'bound to the account');
  const wins = await Promise.all(Array.from({ length: 5 }, () => mfa.consumeMfaChallenge(email, token)));
  assert.equal(wins.filter(Boolean).length, 1, 'consumed exactly once');
  assert.equal(await mfa.checkMfaChallenge(email, token), 'missing');

  const t2 = await mfa.createMfaChallenge(email);
  for (let i = 0; i < 4; i++) assert.equal(await mfa.checkMfaChallenge(email, t2), 'ok');
  assert.equal(await mfa.checkMfaChallenge(email, t2), 'ok', '5th attempt still allowed');
  assert.equal(await mfa.checkMfaChallenge(email, t2), 'locked', '6th attempt locks it');
  assert.equal(await mfa.checkMfaChallenge(email, t2), 'missing', 'and it is gone');

  const t3 = await mfa.createMfaChallenge(email);
  await sql.query(`UPDATE email_codes SET expires_at = now() - interval '1 second' WHERE kind = 'mfa' AND email = $1`, [email]);
  assert.equal(await mfa.checkMfaChallenge(email, t3), 'missing', 'expires after 5 minutes');
  assert.equal((await store.checkLoginCode(email, t3)).result, 'missing', 'not usable as an email code either');
});

test('mfa: step-up window (sign-in counts; 10 minutes; confirm renews it)', async () => {
  const u = await store.createUser('mfa-stepup@x.com', 'h', 'S');
  const { token, session } = await newSession(u);
  assert.ok(session.reauthAt && Date.now() - session.reauthAt < 5000, 'signing in counts as confirming');
  assert.equal(mfa.isRecentlyAuthenticated(session), true);
  await sql.query(`UPDATE sessions SET reauth_at = now() - interval '11 minutes' WHERE id = $1`, [session.id]);
  let s = (await sessions.lookupSession(token)).session;
  assert.equal(mfa.isRecentlyAuthenticated(s), false, 'stale after 10 minutes');
  await mfa.markReauthenticated(session.id);
  s = (await sessions.lookupSession(token)).session;
  assert.equal(mfa.isRecentlyAuthenticated(s), true);
  assert.equal(mfa.isRecentlyAuthenticated({ reauthAt: null }), false);
});

test('mfa: who must keep two-step login on', async () => {
  const plain = await store.createUser('mfa-plain@x.com', 'h', 'P', false);
  assert.equal(await mfa.twoFactorRequirement(plain), null, 'personal-workspace-only user may turn it off');
  assert.match(await mfa.twoFactorRequirement(await store.findUser('admin@x.com')), /admin/);
  const owner = await store.createUser('mfa-teamowner@x.com', 'h', 'T');
  await workspaces.createWorkspace(owner.id, 'MFA Team');
  assert.match(await mfa.twoFactorRequirement(owner), /owners of team workspaces/);
  const keyer = await store.createUser('mfa-keyer@x.com', 'h', 'K');
  const ws = (await workspaces.listWorkspaces(keyer.id))[0];
  await vault.putSecret(ws.id, 'alpaca_paper', ALPACA, keyer.id);
  assert.match(await mfa.twoFactorRequirement(keyer), /broker API keys/);
});

test('mfa: removing the app deletes the seed and codes; rotation re-encrypts seeds', async () => {
  const u = await store.createUser('mfa-rot@x.com', 'h', 'R');
  const { secret } = await mfa.startTotpEnrollment(u);
  await mfa.confirmTotpEnrollment(u.id, codeFor(secret));
  const NEW = crypto.randomBytes(32).toString('base64');
  const prev = process.env.SECRETS_MASTER_KEY;
  try {
    process.env.SECRETS_MASTER_KEY = `9:${NEW},1:${MASTER_V1}`;
    await vault.rotateMasterKey().catch(() => {}); // workspace keys from older tests may be on other versions
    assert.equal((await sql.query('SELECT master_version FROM user_totp WHERE user_id = $1', [u.id])).rows[0].master_version, 9);
    process.env.SECRETS_MASTER_KEY = `9:${NEW}`;
    assert.equal(await mfa.verifyTotpCode(u.id, codeFor(secret, 1)), true, 'still works with only the new master key');
  } finally {
    process.env.SECRETS_MASTER_KEY = prev;
  }
  assert.equal(await mfa.disableTotp(u.id), true);
  assert.equal(await mfa.disableTotp(u.id), false);
  assert.deepEqual(await mfa.getMfaStatus(u.id), { totp: 'off', totpSince: null, recoveryCodesLeft: 0 });
  await assert.rejects(mfa.regenerateRecoveryCodes(u.id), (e) => e.code === 'not_on');
});

test('mfa: sensitive routes ask for a fresh confirm and two-step login', () => {
  const src = (f) => fs.readFileSync(path.join(root, f), 'utf8');
  const secretsRoute = src('app/api/secrets/[kind]/route.ts');
  assert.equal(secretsRoute.match(/requireRecentAuth\(auth\.session\)/g)?.length, 2, 'add/replace and delete broker keys');
  assert.ok(secretsRoute.includes("requireTwoFactorOn(auth.user, 'adding a broker key')"));
  assert.ok(src('app/api/workspaces/[id]/route.ts').includes('requireRecentAuth(auth.session)'), 'delete workspace');
  assert.ok(src('app/api/workspaces/route.ts').includes("requireTwoFactorOn(record, 'creating a team workspace')"));
  const totpRoute = src('app/api/auth/totp/route.ts');
  assert.equal(totpRoute.match(/requireRecentAuth\(session\)/g)?.length, 3, 'start, regenerate, remove');
  const verify = src('app/api/auth/login/verify/route.ts');
  assert.ok(verify.includes('checkMfaChallenge') && verify.includes('consumeMfaChallenge'), 'authenticator step needs the password-step challenge');
});

// ── M19 task 11: devices ──

test('devices: user agents become readable labels', () => {
  const cases = [
    ['Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36', 'Chrome on macOS', 'desktop'],
    ['Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15', 'Safari on macOS', 'desktop'],
    ['Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36 Edg/129.0.0.0', 'Edge on Windows', 'desktop'],
    ['Mozilla/5.0 (X11; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0', 'Firefox on Linux', 'desktop'],
    ['Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1', 'Safari on iPhone', 'mobile'],
    ['Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/129.0 Mobile/15E148 Safari/604.1', 'Chrome on iPhone', 'mobile'],
    ['Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0 Mobile Safari/537.36', 'Chrome on Android', 'mobile'],
    ['Mozilla/5.0 (Linux; Android 14; SM-X710) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/25.0 Chrome/121.0 Safari/537.36', 'Samsung Internet on Android', 'tablet'],
    ['Mozilla/5.0 (iPad; CPU OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1', 'Safari on iPad', 'tablet'],
    ['Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0 Safari/537.36 OPR/114.0', 'Opera on Windows', 'desktop'],
    ['curl/8.7.1', 'curl on unknown system', 'desktop'],
  ];
  for (const [ua, label, kind] of cases) {
    const d = ua_.describeUserAgent(ua);
    assert.equal(d.label, label, ua);
    assert.equal(d.kind, kind, ua);
  }
  assert.equal(ua_.describeUserAgent(null).label, 'Unknown device');
  assert.equal(ua_.describeUserAgent('').label, 'Unknown device');
});

test('devices: first device of a new account isn\'t "new"; another device is; repeats aren\'t', async () => {
  const u = await store.createUser('dev-owner@x.com', 'h', 'D');
  const laptop = devices.newDeviceToken();
  assert.ok(devices.looksLikeDeviceToken(laptop));
  assert.equal(devices.looksLikeDeviceToken('nope'), false);

  let r = await devices.recordDevice(u.id, laptop, { ip: '203.0.113.1', userAgent: 'UA laptop' });
  assert.deepEqual([r.isNew, r.isFirst], [true, true], 'sign-up device: nobody to warn');
  r = await devices.recordDevice(u.id, laptop, { ip: '203.0.113.2', userAgent: 'UA laptop' });
  assert.deepEqual([r.isNew, r.isFirst], [false, false], 'same browser again');
  const phone = devices.newDeviceToken();
  r = await devices.recordDevice(u.id, phone, { ip: '198.51.100.7', userAgent: 'UA phone' });
  assert.deepEqual([r.isNew, r.isFirst], [true, false], 'a second browser is new → email');

  const other = await store.createUser('dev-other@x.com', 'h', 'O');
  r = await devices.recordDevice(other.id, phone, {});
  assert.deepEqual([r.isNew, r.isFirst], [true, true], 'devices are per account');

  const list = await devices.listDevices(u.id);
  assert.equal(list.length, 2);
  assert.equal(list.find((d) => d.userAgent === 'UA laptop').ip, '203.0.113.2', 'last IP kept');
  const raw = await allRowText('SELECT * FROM known_devices WHERE user_id = $1', [u.id]);
  assert.ok(!raw.includes(laptop) && !raw.includes(phone), 'only keyed hashes stored');
  assert.notEqual(devices.deviceHash(laptop).toString('hex'), crypto.createHash('sha256').update(`device:${laptop}`).digest('hex'));
});

test('devices: session list shows each session\'s device and when it was first seen', async () => {
  const u = await store.findUser('dev-owner@x.com');
  const laptop = devices.newDeviceToken();
  const rec = await devices.recordDevice(u.id, laptop, { userAgent: 'UA x' });
  const { session } = await newSession(u, { deviceHash: rec.hash, userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Chrome/129.0 Safari/537.36' });
  await newSession(u, {}); // a session without a device (e.g. created before this feature)
  const list = await sessions.listSessions(u.id);
  const mine = list.find((s) => s.id === session.id);
  assert.ok(mine.deviceFirstSeenAt && Math.abs(mine.deviceFirstSeenAt - Date.now()) < 10_000);
  assert.ok(list.some((s) => s.deviceFirstSeenAt === null));
});

test('devices: password sign-ins notify on a new device; the email is rate-limited', () => {
  const src = (f) => fs.readFileSync(path.join(root, f), 'utf8');
  assert.ok(src('app/api/auth/login/route.ts').includes('startSession(user, req, { notifyNewDevice: true })'));
  assert.equal(src('app/api/auth/login/verify/route.ts').match(/notifyNewDevice: true/g)?.length, 2, 'email code and authenticator paths');
  const sess = src('lib/server/session.ts');
  assert.ok(sess.includes("action: 'auth.login.new_device'"));
  assert.ok(sess.includes('new-device-email:'), 'rate-limited');
  assert.ok(sess.includes('device.isNew && !device.isFirst'), 'no warning for a brand-new account');
});

// ── M19 task 12: team management ──

const teamFixture = {};
const twoFactorUser = async (email, name) => {
  const u = await store.createUser(email, 'h', name, true);
  return u;
};
const roleOf = async (wsId, userId) => (await sql.query('SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2', [wsId, userId])).rows[0]?.role ?? null;

test('team: invites need a team workspace, a valid email and role, and replace the open one', async () => {
  const owner = await twoFactorUser('team-owner@x.com', 'Olive Owner');
  const personal = (await workspaces.listWorkspaces(owner.id))[0];
  await assert.rejects(team.createInvite(personal.id, 'a@x.com', 'editor', owner.id), (e) => e.status === 400 && /Personal/.test(e.message));
  const ws = await workspaces.createWorkspace(owner.id, 'Team Desk');
  await assert.rejects(team.createInvite(ws.id, 'not-an-email', 'editor', owner.id), (e) => e.status === 400);
  await assert.rejects(team.createInvite(ws.id, 'b@x.com', 'owner', owner.id), (e) => e.status === 400 && /editor or a viewer/.test(e.message));
  await assert.rejects(team.createInvite(ws.id, 'TEAM-OWNER@x.com', 'viewer', owner.id), (e) => e.status === 409, 'already a member');

  const first = await team.createInvite(ws.id, ' New.Member@X.com ', 'viewer', owner.id);
  assert.equal(first.invite.email, 'new.member@x.com');
  assert.match(first.token, /^[A-Za-z0-9_-]{43}$/);
  const second = await team.createInvite(ws.id, 'new.member@x.com', 'editor', owner.id);
  assert.equal((await team.previewInvite(first.token)).status, 'revoked', 'a new invite replaces the old link');
  const list = await team.listInvites(ws.id);
  assert.deepEqual(list.map((i) => [i.email, i.role, i.invitedBy]), [['new.member@x.com', 'editor', 'team-owner@x.com']]);
  const raw = await allRowText('SELECT * FROM workspace_invites WHERE workspace_id = $1', [ws.id]);
  assert.ok(!raw.includes(first.token) && !raw.includes(second.token), 'only keyed hashes stored');
  Object.assign(teamFixture, { owner, ws, token: second.token, inviteId: second.invite.id });
});

test('team: invite preview masks the email; dead links say why', async () => {
  const { token, ws } = teamFixture;
  const p = await team.previewInvite(token);
  assert.equal(p.status, 'open');
  assert.equal(p.workspaceName, 'Team Desk');
  assert.equal(p.email, 'n•••@x.com', 'masked: a forwarded link doesn\'t reveal the address');
  assert.equal(p.invitedBy, 'Olive Owner');
  assert.equal((await team.previewInvite('x'.repeat(43))).status, 'missing');
  assert.equal((await team.previewInvite('short')).status, 'missing');
  const dead = await team.createInvite(ws.id, 'late@x.com', 'viewer', teamFixture.owner.id);
  await sql.query(`UPDATE workspace_invites SET expires_at = now() - interval '1 second' WHERE id = $1`, [dead.invite.id]);
  assert.equal((await team.previewInvite(dead.token)).status, 'expired');
  await assert.rejects(team.acceptInvite(dead.token, await store.createUser('late@x.com', 'h', 'L')), (e) => e.status === 409 && /expired/.test(e.message));
  assert.ok(!(await team.listInvites(ws.id)).some((i) => i.email === 'late@x.com'), 'expired invites aren\'t listed');
});

test('team: only the invited address can accept; single use; never demotes', async () => {
  const { token, ws } = teamFixture;
  const stranger = await store.createUser('stranger-team@x.com', 'h', 'S');
  await assert.rejects(team.acceptInvite(token, stranger), (e) => e.status === 403 && /n•••@x\.com/.test(e.message), 'a forwarded link is useless');
  assert.equal(await roleOf(ws.id, stranger.id), null);

  const member = await store.createUser('new.member@x.com', 'h', 'Nina');
  const results = await Promise.allSettled([team.acceptInvite(token, member), team.acceptInvite(token, member)]);
  assert.equal(results.filter((r) => r.status === 'fulfilled').length, 1, 'single use, even in parallel');
  assert.equal(await roleOf(ws.id, member.id), 'editor');
  assert.equal((await team.previewInvite(token)).status, 'used');

  // Already an editor; a viewer invite doesn't demote them.
  const again = await team.createInvite(ws.id, 'other-address@x.com', 'viewer', teamFixture.owner.id);
  await sql.query(`UPDATE workspace_invites SET email = 'new.member@x.com' WHERE id = $1`, [again.invite.id]);
  const r = await team.acceptInvite(again.token, member);
  assert.equal(r.role, 'editor');
  teamFixture.member = member;

  assert.equal(await team.revokeInvite(ws.id, 'garbage'), false);
  const toCancel = await team.createInvite(ws.id, 'cancel-me@x.com', 'viewer', teamFixture.owner.id);
  assert.equal(await team.revokeInvite(ws.id, toCancel.invite.id), true);
  assert.equal(await team.revokeInvite(ws.id, toCancel.invite.id), false);
  await assert.rejects(team.acceptInvite(toCancel.token, await store.createUser('cancel-me@x.com', 'h', 'C')), (e) => /cancelled/.test(e.message));
});

test('team: roles: last owner can\'t be demoted; new owners need two-step login', async () => {
  const { ws, owner, member } = teamFixture;
  await assert.rejects(team.changeRole(ws.id, owner.id, 'editor'), (e) => e.status === 409 && /at least one owner/.test(e.message));
  await assert.rejects(team.changeRole(ws.id, member.id, 'admin'), (e) => e.status === 400);
  await assert.rejects(team.changeRole(ws.id, crypto.randomUUID(), 'viewer'), (e) => e.status === 404);

  await sql.query('UPDATE users SET two_factor = false WHERE id = $1', [member.id]);
  await assert.rejects(team.changeRole(ws.id, member.id, 'owner'), (e) => /two-step login/.test(e.message));
  await sql.query('UPDATE users SET two_factor = true WHERE id = $1', [member.id]);
  assert.deepEqual(await team.changeRole(ws.id, member.id, 'owner'), { from: 'editor', to: 'owner' });
  assert.deepEqual(await team.changeRole(ws.id, member.id, 'owner'), { from: 'owner', to: 'owner' }, 'no-op');

  // Two owners demote each other at the same moment: the workspace still keeps one.
  const res = await Promise.allSettled([team.changeRole(ws.id, owner.id, 'viewer'), team.changeRole(ws.id, member.id, 'viewer')]);
  assert.equal(res.filter((r) => r.status === 'fulfilled').length, 1, 'exactly one demotion wins');
  assert.equal((await sql.query(`SELECT count(*)::int AS n FROM workspace_members WHERE workspace_id = $1 AND role = 'owner'`, [ws.id])).rows[0].n, 1);
  // Restore: owner = owner, member = editor.
  const ownerNow = (await roleOf(ws.id, owner.id)) === 'owner';
  if (!ownerNow) await team.transferOwnership(ws.id, member.id, owner.id);
  await team.changeRole(ws.id, member.id, 'editor');
  assert.equal(await roleOf(ws.id, owner.id), 'owner');
});

test('team: remove and leave keep an owner; removed members lose access at once', async () => {
  const { ws, owner, member } = teamFixture;
  await assert.rejects(team.removeMember(ws.id, owner.id), (e) => e.status === 409 && /last owner/.test(e.message));
  const { token, session } = await newSession(member);
  await workspaces.setActiveWorkspace(session.id, member.id, ws.id);
  assert.equal((await sessions.lookupSession(token)).workspace.id, ws.id);
  assert.deepEqual(await team.removeMember(ws.id, member.id), { role: 'editor' });
  assert.equal((await sessions.lookupSession(token)).workspace.personal, true, 'falls back to personal on the next request');
  await assert.rejects(team.removeMember(ws.id, member.id), (e) => e.status === 404);
});

test('team: transfer ownership', async () => {
  const { ws, owner } = teamFixture;
  const next = await store.createUser('next-owner@x.com', 'h', 'N', true);
  await assert.rejects(team.transferOwnership(ws.id, owner.id, next.id), (e) => e.status === 404, 'must be a member');
  await sql.query(`INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'viewer')`, [ws.id, next.id]);
  await sql.query('UPDATE users SET two_factor = false WHERE id = $1', [next.id]);
  await assert.rejects(team.transferOwnership(ws.id, owner.id, next.id), (e) => /two-step login/.test(e.message));
  await sql.query('UPDATE users SET two_factor = true WHERE id = $1', [next.id]);
  await assert.rejects(team.transferOwnership(ws.id, next.id, owner.id), (e) => e.status === 403, 'only an owner can transfer');
  await assert.rejects(team.transferOwnership(ws.id, owner.id, owner.id), (e) => e.status === 400);
  await team.transferOwnership(ws.id, owner.id, next.id);
  assert.equal(await roleOf(ws.id, next.id), 'owner');
  assert.equal(await roleOf(ws.id, owner.id), 'editor');
});

test('team: invite links use APP_URL (required in production)', () => {
  const req = new Request('http://evil.example/api/x', { headers: { host: 'evil.example' } });
  const prevUrl = process.env.APP_URL;
  const prevEnv = process.env.NODE_ENV;
  try {
    process.env.APP_URL = 'https://app.spectralabs.dev/';
    assert.equal(appUrlMod.appUrl(req), 'https://app.spectralabs.dev');
    delete process.env.APP_URL;
    process.env.NODE_ENV = 'development';
    assert.equal(appUrlMod.appUrl(req), 'http://evil.example', 'dev: request origin');
    process.env.NODE_ENV = 'production';
    assert.throws(() => appUrlMod.appUrl(req), (e) => e.name === 'AppUrlError', 'production never trusts the Host header');
  } finally {
    if (prevUrl === undefined) delete process.env.APP_URL;
    else process.env.APP_URL = prevUrl;
    process.env.NODE_ENV = prevEnv;
  }
  assert.equal(team.maskEmail('alice@fund.com'), 'a•••@fund.com');
});

test('team: routes check roles, ask for a fresh confirm, and write audit events', () => {
  const src = (f) => fs.readFileSync(path.join(root, f), 'utf8');
  const invites = src('app/api/workspaces/[id]/invites/route.ts');
  assert.ok(invites.includes("requireWorkspace('owner', params.id)") && invites.includes('requireRecentAuth(auth.session)') && invites.includes("'membership.invited'"));
  const members = src('app/api/workspaces/[id]/members/[userId]/route.ts');
  assert.equal(members.match(/requireRecentAuth\(auth\.session\)/g)?.length, 2, 'role change and removal');
  for (const a of ['membership.role_changed', 'membership.removed', 'membership.left']) assert.ok(members.includes(`'${a}'`));
  const transfer = src('app/api/workspaces/[id]/transfer/route.ts');
  assert.ok(transfer.includes('requireRecentAuth(auth.session)') && transfer.includes("'workspace.ownership_transferred'"));
  const accept = src('app/api/invites/[token]/route.ts');
  assert.ok(accept.includes("'membership.added'") && accept.includes('getSession()'));
  assert.ok(src('app/api/workspaces/[id]/invites/[inviteId]/route.ts').includes("'membership.invite_revoked'"));
  assert.match(src('components/landing/NavLink.tsx'), /\\\/invite\\\/\[A-Za-z0-9_-\]\{43\}/, 'sign-in can return to an invite page, and nowhere else new');
});

// ── M19 task 13: admin controls ──

test('admin: disabling signs out everywhere and blocks sessions; re-enabling restores sign-in only', async () => {
  const by = await store.findUser('admin@x.com');
  const u = await store.createUser('adm-target@x.com', 'h', 'Target');
  const a = await newSession(u);
  const b = await newSession(u);
  const off = await store.setUserStatus(u.id, 'disabled', by);
  assert.equal(off.status, 'disabled');
  assert.equal(await sessions.lookupSession(a.token), null);
  assert.equal(await sessions.lookupSession(b.token), null);
  assert.equal((await sql.query('SELECT count(*)::int AS n FROM sessions WHERE user_id = $1 AND revoked_at IS NULL', [u.id])).rows[0].n, 0, 'sessions revoked, not just hidden');
  assert.equal((await store.setUserStatus(u.id, 'disabled', by)).status, 'disabled', 'no-op');

  const on = await store.setUserStatus(u.id, 'active', by);
  assert.equal(on.status, 'active');
  assert.equal(await sessions.lookupSession(a.token), null, 'old sessions stay ended: they must sign in again');
  assert.ok(await sessions.lookupSession((await newSession(on)).token));

  const events = (await auditLog.listAudit({ subjectUserId: u.id })).filter((e) => e.action.startsWith('admin.user.'));
  assert.deepEqual(events.map((e) => e.action), ['admin.user.enabled', 'admin.user.disabled'], 'one event per real change');
  assert.equal(events[1].actorEmail, 'admin@x.com');
  assert.equal(await store.setUserStatus(crypto.randomUUID(), 'disabled', by), undefined);
});

test('admin: the last active admin can\'t be disabled', async () => {
  const admins = (await sql.query(`SELECT email FROM users WHERE platform_role = 'admin' AND status = 'active'`)).rows.map((r) => r.email);
  assert.deepEqual(admins, ['admin@x.com']);
  const me = await store.findUser('admin@x.com');
  await assert.rejects(store.setUserStatus(me.id, 'disabled', me), (e) => e.name === 'LastAdminError');
  // With a second admin, an admin can be disabled.
  await store.createUser('adm-second@x.com', 'h', 'Second', true);
  await store.setPlatformRole('adm-second@x.com', 'admin', me);
  const second = await store.findUser('adm-second@x.com');
  assert.equal((await store.setUserStatus(second.id, 'disabled', me)).status, 'disabled');
  await store.setUserStatus(second.id, 'active', me);
  await store.setPlatformRole('adm-second@x.com', 'user', me);
});

test('admin: account list and detail show what an admin needs and nothing secret', async () => {
  const list = await store.listUsers();
  const row = list.find((x) => x.email === 'adm-target@x.com');
  assert.deepEqual(Object.keys(row).sort(), ['createdAt', 'email', 'id', 'lastActiveAt', 'name', 'role', 'status', 'twoFactor']);
  assert.ok(row.lastActiveAt, 'last activity from sessions');

  const u = await store.findUser('adm-target@x.com');
  const team = await workspaces.createWorkspace((await store.findUser('admin@x.com')).id, 'Admin View Desk');
  await sql.query(`INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'viewer')`, [team.id, u.id]);
  const d = await adminUsers.getAdminUserDetail(u.id);
  assert.equal(d.user.email, 'adm-target@x.com');
  assert.equal(d.user.status, 'active');
  assert.deepEqual(d.workspaces.map((w) => [w.personal, w.role]), [[true, 'owner'], [false, 'viewer']]);
  assert.equal(d.workspaces[1].members, 2);
  assert.ok(d.sessions.length >= 1 && d.sessions.every((s) => typeof s.device === 'string'));
  assert.ok(d.events.some((e) => e.action === 'admin.user.disabled'), 'includes events done TO the user');
  const json = JSON.stringify(d);
  assert.ok(!json.includes('passwordHash') && !json.includes('password_hash') && !json.includes('token'), 'no hashes or tokens');
  assert.equal(await adminUsers.getAdminUserDetail('nope'), null);
  assert.equal(await adminUsers.getAdminUserDetail(crypto.randomUUID()), null);
});

test('admin: routes are admin-only, need a fresh confirm, refuse self-changes, and log views', () => {
  const src = fs.readFileSync(path.join(root, 'app/api/admin/users/[id]/route.ts'), 'utf8');
  assert.equal(src.match(/requireAdmin\(\)/g)?.length, 2, 'GET and PATCH');
  assert.ok(src.includes('requireRecentAuth(auth.session)'));
  assert.ok(src.includes('target.id === auth.user.id'), 'no changing your own account');
  assert.ok(src.includes("'admin.user.viewed'"));
  assert.ok(fs.readFileSync(path.join(root, 'app/api/admin/users/route.ts'), 'utf8').includes('requireAdmin()'));
});

// ── M19 task 14: browser security ──

test('csp: scripts only with the request nonce; no inline or eval in production', () => {
  const nonce = csp.newNonce();
  assert.match(nonce, /^[A-Za-z0-9+/]{22}==$/, '128-bit base64 nonce');
  assert.notEqual(nonce, csp.newNonce(), 'fresh every request');
  const prod = csp.buildCsp({ nonce, dev: false, https: true });
  const directive = (name) => prod.split('; ').find((d) => d.startsWith(`${name} `) || d === name);
  assert.equal(directive('script-src'), `script-src 'self' 'nonce-${nonce}' 'strict-dynamic'`);
  assert.ok(!directive('script-src').includes('unsafe'), "no 'unsafe-inline' or 'unsafe-eval' for scripts");
  assert.equal(directive('object-src'), "object-src 'none'");
  assert.equal(directive('frame-ancestors'), "frame-ancestors 'none'");
  assert.equal(directive('base-uri'), "base-uri 'self'");
  assert.equal(directive('form-action'), "form-action 'self'");
  assert.equal(directive('connect-src'), "connect-src 'self'");
  assert.ok(directive('upgrade-insecure-requests'));
  assert.equal(directive('report-uri'), 'report-uri /api/csp-report');
  const dev = csp.buildCsp({ nonce, dev: true, https: false });
  assert.ok(dev.includes("'unsafe-eval'") && dev.includes('ws:'), 'dev server needs eval + websocket');
  assert.ok(!dev.includes('upgrade-insecure-requests'), 'plain-http dev isn\'t upgraded');
  assert.equal(csp.cspMode(undefined), 'report-only', 'report-only by default');
  assert.equal(csp.cspMode('enforce'), 'enforce');
  assert.equal(csp.cspMode('off'), 'off');
  assert.equal(csp.cspMode('banana'), 'report-only');
});

test('csrf: state changes only from this site or allowed origins', () => {
  const allowed = csp.allowedOrigins({ APP_URL: 'https://app.spectralabs.dev/', ALLOWED_ORIGINS: 'https://admin.spectralabs.dev, not a url ,' });
  assert.deepEqual(allowed, ['https://app.spectralabs.dev', 'https://admin.spectralabs.dev']);
  const check = (o) => csp.isCrossSite({ method: 'POST', host: 'app.spectralabs.dev', secFetchSite: null, ...o }, allowed);
  assert.equal(check({ origin: 'https://app.spectralabs.dev' }), false, 'same host');
  assert.equal(check({ origin: 'https://admin.spectralabs.dev', host: 'api.spectralabs.dev' }), false, 'allowed sibling origin');
  assert.equal(check({ origin: 'https://evil.example' }), true);
  assert.equal(check({ origin: 'https://app.spectralabs.dev.evil.example' }), true, 'no suffix tricks');
  assert.equal(check({ origin: 'null' }), true, '"null" origin (sandboxed iframes, file://)');
  assert.equal(check({ origin: null }), false, 'non-browser clients without Origin');
  assert.equal(check({ origin: null, secFetchSite: 'cross-site' }), true, 'browser says cross-site even without Origin');
  assert.equal(csp.isCrossSite({ method: 'GET', origin: 'https://evil.example', host: 'x', secFetchSite: 'cross-site' }, allowed), false, 'reads aren\'t checked');
  assert.equal(csp.isCrossSite({ method: 'delete', origin: 'https://evil.example', host: 'x', secFetchSite: null }, allowed), true);
});

test('security headers: HSTS only over HTTPS; API responses can never run or be framed', () => {
  const h = require(path.join(root, 'lib/securityHeaders.js'));
  assert.equal(h.hstsHeader({ APP_URL: 'http://localhost:3000' }), null, 'not on plain http');
  assert.equal(h.hstsHeader({}), null);
  assert.deepEqual(h.hstsHeader({ APP_URL: 'https://app.spectralabs.dev' }), { key: 'Strict-Transport-Security', value: 'max-age=31536000' });
  assert.equal(h.hstsHeader({ APP_URL: 'https://x', HSTS: 'off' }), null);
  assert.equal(h.hstsHeader({ HSTS: 'on', HSTS_INCLUDE_SUBDOMAINS: '1' }).value, 'max-age=31536000; includeSubDomains');
  const keys = h.pageHeaders({}).map((x) => x.key);
  for (const k of ['X-Frame-Options', 'X-Content-Type-Options', 'Referrer-Policy', 'Permissions-Policy', 'Cross-Origin-Opener-Policy']) assert.ok(keys.includes(k), k);
  assert.deepEqual(h.apiHeaders.find((x) => x.key === 'Content-Security-Policy'), { key: 'Content-Security-Policy', value: "default-src 'none'; frame-ancestors 'none'" });
});

test('csp wiring: middleware sets the nonce and header; layout scripts carry it; reports are redacted', () => {
  const src = (f) => fs.readFileSync(path.join(root, f), 'utf8');
  const mw = src('middleware.ts');
  assert.ok(mw.includes("requestHeaders.set('x-nonce', nonce)") && mw.includes("requestHeaders.set('content-security-policy', csp)"));
  assert.ok(mw.includes("'Content-Security-Policy-Report-Only'") && mw.includes("'Content-Security-Policy'"));
  assert.ok(mw.includes('pathname !== CSP_REPORT_PATH'), 'reports are exempt from the cross-site guard');
  const layout = src('app/layout.tsx');
  assert.equal(layout.match(/<script nonce=\{nonce\}/g)?.length, 2, 'both inline scripts carry the nonce');
  assert.ok(!/<script(?![^>]*nonce)[^>]*dangerouslySetInnerHTML/.test(layout), 'no un-nonced inline script');
  const report = src('app/api/csp-report/route.ts');
  assert.ok(report.includes("'/invite/…'") && report.includes('split(/[?#]/)'), 'invite tokens and query strings never reach the log');
});

// ── M19 task 9: audit log ──

const fakeReq = (headers = {}) => new Request('http://localhost/x', { headers });

test('audit: events are written with server time, request metadata and a hash chain', async () => {
  const u = await store.createUser('audit-user@x.com', 'h', 'Aud');
  const meta = auditLog.requestMeta(fakeReq({ 'x-forwarded-for': '198.51.100.9, 10.0.0.1', 'user-agent': 'UA/1', 'x-request-id': 'req-123' }));
  assert.deepEqual(meta, { ip: '198.51.100.9', userAgent: 'UA/1', requestId: 'req-123' });
  assert.match(auditLog.requestMeta(fakeReq({ 'x-request-id': 'bad id with spaces' })).requestId, /^[0-9a-f-]{36}$/, 'untrusted request ids are replaced');

  await auditLog.record({ action: 'auth.login.success', actor: u, target: { type: 'session', id: 's1' }, details: { method: 'password' } }, meta);
  const [row] = (await sql.query(`SELECT * FROM audit_events WHERE actor_id = $1`, [u.id])).rows;
  assert.equal(row.action, 'auth.login.success');
  assert.equal(row.actor_email, 'audit-user@x.com');
  assert.equal(row.ip, '198.51.100.9');
  assert.equal(row.request_id, 'req-123');
  assert.deepEqual(row.details, { method: 'password' });
  assert.ok(Math.abs(row.occurred_at - Date.now()) < 10_000, 'time is set by the database');
  assert.equal(row.hash.length, 32);

  const chain = await auditLog.verifyChain();
  assert.equal(chain.ok, true);
  assert.equal(chain.head, (await sql.query('SELECT encode(hash, $1) AS h FROM audit_events ORDER BY seq DESC LIMIT 1', ['hex'])).rows[0].h);
});

test('audit: parallel writers still form one unbroken chain', async () => {
  const before = Number((await sql.query('SELECT coalesce(max(seq), 0) AS m FROM audit_events')).rows[0].m);
  await Promise.all(Array.from({ length: 25 }, (_, i) => auditLog.record({ action: 'auth.logout', details: { i } })));
  const seqs = (await sql.query('SELECT seq FROM audit_events WHERE seq > $1 ORDER BY seq', [before])).rows.map((r) => Number(r.seq));
  assert.deepEqual(seqs, Array.from({ length: 25 }, (_, i) => before + i + 1), 'seq is contiguous');
  assert.equal((await auditLog.verifyChain()).ok, true);
});

test('audit: append-only (UPDATE, DELETE, TRUNCATE refused); a failed write never throws', async () => {
  await assert.rejects(sql.query(`UPDATE audit_events SET action = 'auth.logout'`), /append-only/);
  await assert.rejects(sql.query('DELETE FROM audit_events'), /append-only/);
  await assert.rejects(sql.query('TRUNCATE audit_events'), /append-only/);

  const errors = [];
  const orig = console.error;
  console.error = (...a) => errors.push(a.join(' '));
  try {
    await auditLog.record({ action: 'NOT VALID' });
  } finally {
    console.error = orig;
  }
  assert.match(errors.join('\n'), /AUDIT WRITE FAILED/, 'logged loudly instead of failing the user action');

  await auditLog.record({ action: 'auth.logout', details: { blob: 'x'.repeat(10_000) } });
  const [last] = (await sql.query('SELECT details FROM audit_events ORDER BY seq DESC LIMIT 1')).rows;
  assert.deepEqual(last.details, { truncated: true }, 'oversized details are capped');
  for (const a of auditLog.AUDIT_ACTIONS) assert.match(a, /^[a-z0-9_]+(\.[a-z0-9_]+)+$/, `${a} fits the database check`);
});

test('audit: filters, paging, workspace scoping', async () => {
  const owner = await store.createUser('audit-ws@x.com', 'h', 'W');
  const ws = await workspaces.createWorkspace(owner.id, 'Audit Desk');
  const other = await workspaces.createWorkspace(owner.id, 'Other Desk');
  for (let i = 0; i < 5; i++) await auditLog.record({ action: 'secret.used', actor: owner, workspaceId: ws.id, details: { i } });
  await auditLog.record({ action: 'secret.deleted', actor: owner, workspaceId: other.id });
  await auditLog.record({ action: 'auth.login.failure', outcome: 'failure', actor: { email: 'odd_name%@x.com' } });

  const scoped = await auditLog.listAudit({ workspaceId: ws.id });
  assert.equal(scoped.length, 5);
  assert.ok(scoped.every((e) => e.workspaceId === ws.id && e.workspaceName === 'Audit Desk'));
  assert.ok(Number(scoped[0].seq) > Number(scoped[4].seq), 'newest first');

  const page1 = await auditLog.listAudit({ workspaceId: ws.id, limit: 2 });
  const page2 = await auditLog.listAudit({ workspaceId: ws.id, limit: 2, beforeSeq: page1[1].seq });
  assert.equal(page2.length, 2);
  assert.ok(Number(page2[0].seq) < Number(page1[1].seq), 'cursor paging');

  assert.ok((await auditLog.listAudit({ action: 'secret.' })).every((e) => e.action.startsWith('secret.')), 'prefix filter');
  assert.ok((await auditLog.listAudit({ action: 'secret.deleted' })).every((e) => e.action === 'secret.deleted'));
  assert.equal((await auditLog.listAudit({ action: 'secret_' })).length, 0, 'LIKE wildcards are escaped');
  const failures = await auditLog.listAudit({ outcome: 'failure' });
  assert.ok(failures.length >= 1 && failures.every((e) => e.outcome === 'failure'));
  assert.equal((await auditLog.listAudit({ actor: 'ODD_NAME%' })).length, 1, 'actor search is case-insensitive and escaped');
  assert.equal((await auditLog.listAudit({ actor: 'odd_name%', from: new Date(Date.now() + 60_000) })).length, 0, 'date filter');

  const f = auditLog.filterFromQuery(new URLSearchParams('action=auth.&outcome=nope&actor=%20a%20&from=garbage&limit=5&before=12'));
  assert.deepEqual(f, { action: 'auth.', outcome: undefined, actor: 'a', from: undefined, to: undefined, beforeSeq: '12', limit: 5 });
  assert.equal(auditLog.filterFromQuery(new URLSearchParams("action=x' OR 1=1")).action, undefined, 'odd input is dropped');
});

test('audit: CSV export defuses spreadsheet formulas and quotes properly', () => {
  assert.equal(auditLog.csvField('=HYPERLINK("http://evil")'), `"'=HYPERLINK(""http://evil"")"`);
  assert.equal(auditLog.csvField('+1'), "'+1");
  assert.equal(auditLog.csvField('-cmd'), "'-cmd");
  assert.equal(auditLog.csvField('@SUM(A1)'), "'@SUM(A1)");
  assert.equal(auditLog.csvField('a,b'), '"a,b"');
  assert.equal(auditLog.csvField('line\nbreak'), '"line\nbreak"');
  assert.equal(auditLog.csvField(null), '');
  assert.equal(auditLog.csvField('plain'), 'plain');
  const csv = auditLog.toCsv([
    { seq: '1', occurredAt: 0, action: 'auth.logout', outcome: 'success', actorEmail: '=evil@x.com', actorId: null, workspaceName: null, workspaceId: null, targetType: null, targetId: null, ip: null, userAgent: 'UA, "quoted"', requestId: null, details: {}, hash: 'ab' },
  ]);
  const [header, line] = csv.trim().split('\r\n');
  assert.ok(header.startsWith('seq,time_utc,action,outcome,actor_email'));
  assert.ok(line.includes("'=evil@x.com"));
  assert.ok(line.includes('"UA, ""quoted"""'));
});

test('audit: role changes, secret use and the admin CLI are recorded', async () => {
  const by = await store.findUser('admin@x.com');
  await store.createUser('audit-promote@x.com', 'h', 'P');
  await store.setPlatformRole('audit-promote@x.com', 'admin', by);
  await store.setPlatformRole('audit-promote@x.com', 'admin', by); // no change → no event
  await store.setPlatformRole('audit-promote@x.com', 'user', by);
  const roleEvents = (await auditLog.listAudit({ action: 'admin.role.' })).filter((e) => e.details.email === 'audit-promote@x.com');
  assert.deepEqual(roleEvents.map((e) => e.action), ['admin.role.revoked', 'admin.role.granted']);
  assert.equal(roleEvents[0].actorEmail, 'admin@x.com');

  const owner = await store.createUser('audit-vault@x.com', 'h', 'AV');
  const ws = (await workspaces.listWorkspaces(owner.id))[0];
  await vault.putSecret(ws.id, 'alpaca_paper', ALPACA, owner.id);
  await vault.useSecret(ws.id, 'alpaca_paper', async () => 'ok', { actor: owner, purpose: 'order' });
  const [used] = await auditLog.listAudit({ workspaceId: ws.id, action: 'secret.used' });
  assert.equal(used.actorEmail, 'audit-vault@x.com');
  assert.deepEqual(used.details, { purpose: 'order' });
  assert.ok(!JSON.stringify(await auditLog.listAudit({ workspaceId: ws.id })).includes(ALPACA.secretKey), 'no key material in the log');

  const { execFileSync } = await import('node:child_process');
  await store.createUser('audit-cli@x.com', 'h', 'C');
  execFileSync(process.execPath, [path.join(root, 'scripts/admin.mjs'), 'grant', 'audit-cli@x.com'], { stdio: 'ignore', env: { ...process.env, DATABASE_URL: testUrl, USER: 'amogh' } });
  const [cli] = await auditLog.listAudit({ action: 'admin.role.granted', actor: 'cli:' });
  assert.equal(cli.actorEmail, 'cli:amogh');
  assert.equal(cli.details.email, 'audit-cli@x.com');
  await store.setPlatformRole('audit-cli@x.com', 'user', by);
});

test('audit: the routes that change security state write events', () => {
  const expect = {
    'app/api/auth/login/route.ts': ['auth.login.success', 'auth.login.failure', 'auth.login.locked'],
    'app/api/auth/login/verify/route.ts': ['auth.login.success', 'auth.login.failure'],
    'app/api/auth/verify-otp/route.ts': ['auth.signup', 'auth.login.success'],
    'app/api/auth/logout/route.ts': ['auth.logout'],
    'app/api/auth/2fa/route.ts': ['auth.two_factor.enabled', 'auth.two_factor.disabled'],
    'app/api/auth/password/route.ts': ['auth.password.changed'],
    'app/api/auth/forgot/route.ts': ['auth.password.reset_requested'],
    'app/api/auth/reset/route.ts': ['auth.password.reset'],
    'app/api/auth/sessions/route.ts': ['auth.session.revoked', 'auth.session.revoked_others'],
    'app/api/auth/account/route.ts': ['account.deleted'],
    'app/api/workspaces/route.ts': ['workspace.created'],
    'app/api/workspaces/[id]/route.ts': ['workspace.renamed', 'workspace.deleted'],
    'app/api/secrets/[kind]/route.ts': ['secret.added', 'secret.replaced', 'secret.deleted'],
    'lib/server/vault.ts': ['secret.used'],
    'lib/server/authStore.ts': ['admin.role.granted', 'admin.role.revoked'],
    'lib/server/auditRoute.ts': ['audit.exported'],
  };
  for (const [file, actions] of Object.entries(expect)) {
    const src = fs.readFileSync(path.join(root, file), 'utf8');
    for (const a of actions) assert.ok(src.includes(`'${a}'`), `${file} records ${a}`);
  }
});

// ── M19 task 18: permission rules for every API route ──

/**
 * Every exported handler under app/api, with the guard it must call first. A new route (or a new
 * method on an old one) fails this test until it is added here, so nobody ships an unguarded route
 * by accident. Guards: public (no sign-in; must be rate-limited if it does anything), session
 * (optional sign-in via getSession), user (requireUser), admin (requireAdmin), engine (role from
 * the engine route table), or a workspace role; "@url" = checked against the workspace in the URL.
 */
const ROUTE_RULES = {
  'admin/audit GET': 'admin',
  'admin/audit/verify GET': 'admin',
  'admin/users GET': 'admin',
  'admin/users/[id] GET': 'admin',
  'admin/users/[id] PATCH': 'admin',
  'auth/2fa GET': 'user',
  'auth/2fa POST': 'user',
  'auth/account GET': 'user',
  'auth/account PATCH': 'user',
  'auth/account DELETE': 'user',
  'auth/forgot POST': 'public',
  'auth/login POST': 'public',
  'auth/login/verify POST': 'public',
  'auth/logout POST': 'session',
  'auth/password POST': 'user',
  'auth/reauth POST': 'user',
  'auth/reset POST': 'public',
  'auth/send-otp POST': 'public',
  'auth/session GET': 'session',
  'auth/sessions GET': 'user',
  'auth/sessions DELETE': 'user',
  'auth/totp GET': 'user',
  'auth/totp POST': 'user',
  'auth/totp DELETE': 'user',
  'auth/verify-otp POST': 'public',
  'csp-report POST': 'public',
  'engine/[...path] GET': 'engine',
  'engine/[...path] POST': 'engine',
  'engine/[...path] PATCH': 'engine',
  'engine/[...path] DELETE': 'engine',
  'engine-upload POST': 'editor',
  'invites/[token] GET': 'public',
  'invites/[token] POST': 'session',
  'secrets GET': 'viewer',
  'secrets/[kind] PUT': 'owner',
  'secrets/[kind] DELETE': 'owner',
  'workspaces GET': 'user',
  'workspaces POST': 'user',
  'workspaces/active PUT': 'viewer',
  'workspaces/[id] GET': 'viewer@url',
  'workspaces/[id] PATCH': 'owner@url',
  'workspaces/[id] DELETE': 'owner@url',
  'workspaces/[id]/audit GET': 'owner@url',
  'workspaces/[id]/invites GET': 'owner@url',
  'workspaces/[id]/invites POST': 'owner@url',
  'workspaces/[id]/invites/[inviteId] DELETE': 'owner@url',
  'workspaces/[id]/members/[userId] PATCH': 'owner@url',
  'workspaces/[id]/members/[userId] DELETE': 'viewer@url', // leaving; removing others is owner-only (checked below)
  'workspaces/[id]/transfer POST': 'owner@url',
  'workspaces/[id]/tokens GET': 'viewer@url', // owners see every token, others their own (apiTokens.listTokens)
  'workspaces/[id]/tokens POST': 'viewer@url', // personal tokens; workspace tokens and "run" checked in createToken
  'workspaces/[id]/tokens/[tokenId] DELETE': 'viewer@url', // your own; owners any (apiTokens.revokeToken)
};
// Workspace routes that change something yet only need viewer: switching your own active
// workspace, and leaving one. Everything else that changes workspace data needs editor or more.
const VIEWER_WRITES = new Set(['workspaces/active PUT', 'workspaces/[id]/members/[userId] DELETE', 'workspaces/[id]/tokens POST', 'workspaces/[id]/tokens/[tokenId] DELETE']);
const STATE_CHANGING = new Set(['POST', 'PUT', 'PATCH', 'DELETE']);

/** Every route handler: { key: 'path METHOD', file, body } where body includes local helpers it calls. */
function apiHandlers() {
  const apiRoot = path.join(root, 'app/api');
  const files = [];
  const walk = (dir) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.name === 'route.ts' || e.name === 'route.js') files.push(p);
    }
  };
  walk(apiRoot);
  const out = [];
  for (const file of files) {
    const src = fs.readFileSync(file, 'utf8');
    const route = path.relative(apiRoot, path.dirname(file)).split(path.sep).join('/');
    // Top-level declarations: from one "\n(export )?(async )?function|const" to the next.
    const decls = [...src.matchAll(/^(?:export\s+)?(?:async\s+)?(?:function\s+(\w+)|const\s+(\w+)\s*=)/gm)].map((m, i, all) => ({
      name: m[1] || m[2],
      exported: m[0].startsWith('export'),
      text: src.slice(m.index, all[i + 1]?.index ?? src.length),
    }));
    const local = new Map(decls.map((d) => [d.name, d.text]));
    for (const d of decls) {
      if (!d.exported || !['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'].includes(d.name)) continue;
      let body = d.text;
      for (const [, callee] of d.text.matchAll(/\b(\w+)\(/g)) if (callee !== d.name && local.has(callee)) body += '\n' + local.get(callee);
      out.push({ key: `${route} ${d.name}`, method: d.name, file: path.relative(root, file), body });
    }
    assert.ok(!/export\s+\{/.test(src) && !/export\s+const\s+(GET|POST|PUT|PATCH|DELETE)\b/.test(src), `${route}: export handlers as plain functions so this test can see them`);
  }
  return out;
}

/** The first guard a handler calls, in ROUTE_RULES form. */
function guardOf(body) {
  const guards = [
    [/requireAdmin\(/, () => 'admin'],
    [/requireWorkspace\(minRole/, () => 'engine'],
    [/requireWorkspace\('(\w+)'(?:,\s*(params\.id))?/, (m) => m[1] + (m[2] ? '@url' : '')],
    [/requireUser\(/, () => 'user'],
    [/getSession\(/, () => 'session'],
  ];
  let best = null;
  for (const [re, name] of guards) {
    const m = re.exec(body);
    if (m && (!best || m.index < best.index)) best = { index: m.index, name: name(m) };
  }
  return best?.name ?? 'public';
}

test('permissions: every API route has a rule, and its handler calls that guard first', () => {
  const handlers = apiHandlers();
  const seen = new Set(handlers.map((h) => h.key));
  const unlisted = [...seen].filter((k) => !(k in ROUTE_RULES));
  assert.deepEqual(unlisted, [], 'new API route(s): add a rule to ROUTE_RULES in scripts/test-auth.mjs');
  const stale = Object.keys(ROUTE_RULES).filter((k) => !seen.has(k));
  assert.deepEqual(stale, [], 'ROUTE_RULES lists routes that no longer exist');
  for (const h of handlers) assert.equal(guardOf(h.body), ROUTE_RULES[h.key], `${h.key} (${h.file})`);
});

test('permissions: viewer = read only, editor = create and run, owner = members, settings, delete', () => {
  const rank = { viewer: 1, editor: 2, owner: 3 };
  for (const [key, rule] of Object.entries(ROUTE_RULES)) {
    const role = rule.replace('@url', '');
    if (!(role in rank)) continue;
    const method = key.split(' ').pop();
    if (STATE_CHANGING.has(method) && !VIEWER_WRITES.has(key)) assert.ok(rank[role] >= rank.editor, `${key}: changes workspace data, so it needs editor or owner (has ${role})`);
    if (/^workspaces\/\[id\]/.test(key)) assert.ok(rule.endsWith('@url'), `${key}: must check the workspace named in the URL, not the active one`);
    const ownerOnly = /^workspaces\/\[id\]\/(invites|members|transfer|audit)/.test(key) || (/^(secrets|workspaces\/\[id\] )/.test(key) && STATE_CHANGING.has(method));
    if (ownerOnly && key !== 'workspaces/[id]/members/[userId] DELETE') {
      assert.equal(role, 'owner', `${key}: members, broker keys, settings and the activity log are owner-only`);
    }
  }
  // Engine: computations need editor, reads need viewer (the table itself is tested in task 4's tests).
  for (const [p, r] of Object.entries(engineRoutes.ENGINE_ROUTES)) {
    const methods = Array.isArray(r.method) ? r.method : [r.method];
    for (const method of methods) {
      if (method !== 'GET' && !r.public) assert.ok(rank[engineRoutes.minimumRole(r, method)] >= rank.editor, `engine ${method} ${p}`);
    }
  }

  // The viewer-level writes stay narrow.
  const handlers = Object.fromEntries(apiHandlers().map((h) => [h.key, h.body]));
  const leave = handlers['workspaces/[id]/members/[userId] DELETE'];
  assert.match(leave, /const leaving = params\.userId === auth\.user\.id/);
  assert.match(leave, /if \(!leaving\) \{\s*if \(workspace\.role !== 'owner'\)/, 'removing someone else is owner-only');
  assert.match(handlers['workspaces/active PUT'], /setActiveWorkspace\(auth\.session\.id, auth\.user\.id/, 'switching only moves your own session');
  assert.match(handlers['workspaces/[id]/tokens POST'], /createToken\(\{ by: auth\.user, workspace,/, 'token rules are enforced with the caller\'s role (apiTokens tests)');
  assert.match(handlers['workspaces/[id]/tokens/[tokenId] DELETE'], /revokeToken\(workspace, params\.tokenId, auth\.user\)/);
});

test('permissions: admin routes, public routes and sensitive changes', () => {
  const handlers = apiHandlers();
  for (const h of handlers) {
    const rule = ROUTE_RULES[h.key];
    if (h.key.startsWith('admin/')) assert.equal(rule, 'admin', `${h.key}: everything under /api/admin is admin-only`);
    if (rule === 'admin' && STATE_CHANGING.has(h.method)) assert.match(h.body, /requireRecentAuth\(/, `${h.key}: admin changes need a fresh confirm`);
    // Anything reachable without an account must be rate-limited (except CSP reports, which only log).
    if ((rule === 'public' || rule === 'session') && h.key !== 'csp-report POST' && h.key !== 'auth/session GET' && h.key !== 'auth/logout POST') {
      assert.match(h.body, /\bhit(All)?\(/, `${h.key}: public route without a rate limit`);
    }
    // Owner-level changes to members, keys and workspaces ask for a fresh confirm.
    // (Exempt: renaming a workspace and cancelling an invite only reduce or don't change access.)
    if (/^owner/.test(rule) && STATE_CHANGING.has(h.method) && !['workspaces/[id] PATCH', 'workspaces/[id]/invites/[inviteId] DELETE'].includes(h.key)) {
      assert.match(h.body, /requireRecentAuth\(auth\.session\)/, `${h.key}: needs a fresh confirm`);
    }
  }
  const csp = handlers.find((h) => h.key === 'csp-report POST').body;
  assert.doesNotMatch(csp, /query\(|record\(|audit\(/, 'CSP reports touch nothing but the log');
});

// ── M19 task 18: spike alerts ──

test('spikes: thresholds come from "count/minutes", "off", or the defaults', () => {
  const spikes = load('lib/server/spikes.js');
  const warn = console.warn;
  console.warn = () => {};
  try {
    assert.deepEqual(spikes.spikeThreshold('failed_login', {}), { max: 50, windowMs: 600_000 });
    assert.deepEqual(spikes.spikeThreshold('code_request', {}), { max: 100, windowMs: 600_000 });
    assert.deepEqual(spikes.spikeThreshold('failed_login', { ALERT_FAILED_LOGINS: ' 20 / 5 ' }), { max: 20, windowMs: 300_000 });
    assert.deepEqual(spikes.spikeThreshold('code_request', { ALERT_CODE_REQUESTS: '30/15m' }), { max: 30, windowMs: 900_000 });
    assert.equal(spikes.spikeThreshold('failed_login', { ALERT_FAILED_LOGINS: 'OFF' }), null);
    for (const bad of ['0/10', '10/0', 'lots', '-5/10', '10'])
      assert.deepEqual(spikes.spikeThreshold('failed_login', { ALERT_FAILED_LOGINS: bad }), { max: 50, windowMs: 600_000 }, bad);
  } finally {
    console.warn = warn;
  }
});

test('spikes: one alert when failed sign-ins cross the line, with sources; then quiet for an hour', async () => {
  const spikes = load('lib/server/spikes.js');
  const sent = [];
  spikes.setAlertNotifier(async (alert, to) => void sent.push({ alert, to }));
  const env = { ...process.env };
  process.env.ALERT_FAILED_LOGINS = '5/10';
  delete process.env.ALERT_EMAIL;
  const warn = console.warn;
  const logged = [];
  console.warn = (...a) => logged.push(a.join(' '));
  try {
    await sql.query(`DELETE FROM rate_limits WHERE key LIKE 'spike:%'`);
    const meta = (ip) => ({ ip, userAgent: 'test', requestId: crypto.randomUUID() });
    for (let i = 0; i < 3; i++) await auditLog.record({ action: 'auth.login.failure', outcome: 'failure', actor: { email: `spike${i}@x.com` }, details: { reason: 'wrong_password' } }, meta('198.51.100.7'));
    await auditLog.record({ action: 'auth.login.failure', outcome: 'failure', actor: { email: 'spike9@x.com' } }, meta('203.0.113.9'));

    for (let i = 1; i < 5; i++) assert.equal(await spikes.noteSpike('failed_login'), null, `event ${i} is under the threshold`);
    const alert = await spikes.noteSpike('failed_login');
    assert.ok(alert, 'the 5th event alerts');
    assert.equal(alert.kind, 'failed_login');
    assert.equal(alert.count, 5);
    assert.equal(alert.windowMinutes, 10);
    assert.deepEqual(alert.topIps.find((t) => t.ip === '198.51.100.7'), { ip: '198.51.100.7', count: 3 });
    assert.ok(alert.accounts >= 4);
    assert.match(spikes.summarize(alert), /^5 failed sign-ins in the last 10 minutes \(site-wide\)\. \d+ accounts targeted\. Top sources: .*198\.51\.100\.7 \(3\)/);

    for (let i = 0; i < 10; i++) assert.equal(await spikes.noteSpike('failed_login'), null, 'no repeat within the hour');
    assert.equal(sent.length, 1);
    const admins = (await sql.query(`SELECT email FROM users WHERE platform_role = 'admin' AND status = 'active' ORDER BY email`)).rows.map((r) => r.email);
    assert.ok(admins.length > 0);
    assert.deepEqual(sent[0].to, admins, 'goes to every active admin by default');
    assert.match(logged.join('\n'), /\[SECURITY ALERT\] 5 failed sign-ins/);

    const [ev] = (await sql.query(`SELECT outcome, details FROM audit_events WHERE action = 'security.alert' ORDER BY seq DESC LIMIT 1`)).rows;
    assert.equal(ev.outcome, 'failure');
    assert.equal(ev.details.kind, 'failed_login');
    assert.equal(ev.details.count, 5);

    // After the cooldown, a new spike alerts again.
    await sql.query(`UPDATE rate_limits SET reset_at = now() - interval '1 second' WHERE key LIKE 'spike:failed_login%'`);
    for (let i = 1; i < 5; i++) await spikes.noteSpike('failed_login');
    assert.ok(await spikes.noteSpike('failed_login'));
    assert.equal(sent.length, 2);
  } finally {
    console.warn = warn;
    spikes.setAlertNotifier(null);
    for (const k of ['ALERT_FAILED_LOGINS', 'ALERT_EMAIL']) env[k] === undefined ? delete process.env[k] : (process.env[k] = env[k]);
  }
});

test('spikes: code requests, ALERT_EMAIL, "off", and failures never reach the caller', async () => {
  const spikes = load('lib/server/spikes.js');
  const env = { ...process.env };
  const warn = console.warn;
  const err = console.error;
  const errors = [];
  console.warn = () => {};
  console.error = (...a) => errors.push(a.join(' '));
  try {
    assert.deepEqual(await spikes.alertRecipients({ ALERT_EMAIL: ' Sec@X.com, not-an-email, sec@x.com,ops@x.com ' }), ['sec@x.com', 'ops@x.com']);

    await sql.query(`DELETE FROM rate_limits WHERE key LIKE 'spike:%'`);
    process.env.ALERT_CODE_REQUESTS = '3/10';
    process.env.ALERT_EMAIL = 'oncall@x.com';
    let to = null;
    spikes.setAlertNotifier(async (_a, r) => {
      to = r;
      throw new Error('smtp down');
    });
    await spikes.noteSpike('code_request');
    await spikes.noteSpike('code_request');
    const alert = await spikes.noteSpike('code_request');
    assert.equal(alert.kind, 'code_request');
    assert.equal(alert.topIps, undefined, 'no per-IP data for code requests');
    assert.deepEqual(to, ['oncall@x.com']);
    assert.match(errors.join('\n'), /notification failed: smtp down/, 'a failed notification is logged, not thrown');

    process.env.ALERT_CODE_REQUESTS = 'off';
    await sql.query(`DELETE FROM rate_limits WHERE key LIKE 'spike:%'`);
    for (let i = 0; i < 5; i++) assert.equal(await spikes.noteSpike('code_request'), null);
    assert.equal((await sql.query(`SELECT count(*)::int AS n FROM rate_limits WHERE key LIKE 'spike:%'`)).rows[0].n, 0, '"off" counts nothing');

    // A broken database never breaks sign-in.
    process.env.ALERT_CODE_REQUESTS = '3/10';
    await sql.query('ALTER TABLE rate_limits RENAME TO rate_limits_away');
    try {
      assert.equal(await spikes.noteSpike('code_request'), null);
    } finally {
      await sql.query('ALTER TABLE rate_limits_away RENAME TO rate_limits');
    }
    assert.match(errors.join('\n'), /spike counter \(code_request\) failed/);
  } finally {
    console.warn = warn;
    console.error = err;
    spikes.setAlertNotifier(null);
    for (const k of ['ALERT_CODE_REQUESTS', 'ALERT_EMAIL']) env[k] === undefined ? delete process.env[k] : (process.env[k] = env[k]);
  }
});

test('spikes: every failed sign-in and every code request is counted', () => {
  const src = (f) => fs.readFileSync(path.join(root, f), 'utf8');
  for (const f of ['app/api/auth/login/route.ts', 'app/api/auth/login/verify/route.ts']) {
    const s = src(f);
    const failures = s.match(/action: 'auth\.login\.failure'[^\n]*\n\s*await noteSpike\('failed_login'\);/g)?.length ?? 0;
    assert.equal(failures, s.match(/action: 'auth\.login\.failure'/g).length, `${f}: each failed sign-in is followed by noteSpike`);
  }
  for (const [f, before] of [
    ['app/api/auth/send-otp/route.ts', '`otp:ip:'],
    ['app/api/auth/forgot/route.ts', '`reset:ip:'],
    ['app/api/auth/login/route.ts', '`login-otp:email:burst:'],
  ]) {
    const s = src(f);
    const i = s.indexOf("await noteSpike('code_request');");
    assert.ok(i > 0 && i < s.indexOf(before), `${f}: code requests are counted before the per-user limits (refused ones count too)`);
  }
});

// ── M19 task 17: API tokens ──

const tok = () => load('lib/server/apiTokens.js');
const membership = (userId, wsId) => workspaces.getMembership(userId, wsId);
const teamLib = () => team; // (tests below name their workspaces "team")

test('api tokens: shown once, only a keyed hash stored, act in their workspace', async () => {
  const t = tok();
  const owner = await store.createUser('tok-owner@x.com', 'h', 'Owner');
  const team = await workspaces.createWorkspace(owner.id, 'Token Desk');
  const { token, info } = await t.createToken({ by: owner, workspace: await membership(owner.id, team.id), kind: 'personal', name: '  research   notebook ', scopes: ['read'], expiresInDays: 30 });
  assert.match(token, /^spectra_pat_[A-Za-z0-9_-]{43}$/);
  assert.match(token, t.TOKEN_RE);
  assert.equal(info.name, 'research notebook');
  assert.deepEqual(info.scopes, ['read']);
  assert.equal(info.hint, token.slice(-4));
  assert.equal(info.owner.email, 'tok-owner@x.com');
  assert.ok(Math.abs(info.expiresAt - (Date.now() + 30 * 86_400_000)) < 60_000);

  const raw = JSON.stringify((await sql.query('SELECT * FROM api_tokens WHERE id = $1', [info.id])).rows);
  assert.ok(!raw.includes(token) && !raw.includes(token.slice(12)), 'the token itself is never stored');

  const auth = await t.authenticateToken(token, '198.51.100.20');
  assert.equal(auth.token.id, info.id);
  assert.equal(auth.user.id, owner.id);
  assert.equal(auth.workspace.id, team.id);
  assert.equal(auth.workspace.role, 'viewer', 'an owner\'s read token acts as a viewer');
  const [row] = (await sql.query('SELECT last_used_at, last_used_ip FROM api_tokens WHERE id = $1', [info.id])).rows;
  assert.ok(row.last_used_at && row.last_used_ip === '198.51.100.20', 'last use is recorded');

  for (const bad of [null, '', 'nope', token + 'x', token.replace('spectra_pat_', 'spectra_wat_'), 'spectra_pat_' + 'A'.repeat(43)]) {
    assert.equal(await t.authenticateToken(bad), null, `refused: ${String(bad).slice(0, 20)}`);
  }
  assert.equal(t.bearerToken(null), undefined);
  assert.equal(t.bearerToken(`Bearer ${token}`), token);
  assert.equal(t.bearerToken(`bearer   ${token} `), token);
  assert.equal(t.bearerToken('Basic abc'), '', 'any other Authorization header is a failed token, never a fallback to the cookie');
});

test('api tokens: who can make which token, and validation', async () => {
  const t = tok();
  const owner = await store.createUser('tok-rules-owner@x.com', 'h', 'O');
  const editor = await store.createUser('tok-rules-editor@x.com', 'h', 'E');
  const viewer = await store.createUser('tok-rules-viewer@x.com', 'h', 'V');
  const team = await workspaces.createWorkspace(owner.id, 'Rules Desk');
  await addMember(team.id, editor.id, 'editor');
  await addMember(team.id, viewer.id, 'viewer');
  const make = async (u, fields) => t.createToken({ by: u, workspace: await membership(u.id, team.id), name: 'n', scopes: ['read'], ...fields });
  const status = (p) => p.then(() => 200, (e) => (e.name === 'TokenError' ? e.status : Promise.reject(e)));

  assert.equal(await status(make(viewer, { scopes: ['run'] })), 403, 'viewers: read-only tokens');
  assert.equal(await status(make(viewer, {})), 200);
  assert.deepEqual((await make(editor, { scopes: ['run'] })).info.scopes, ['read', 'run'], 'run always includes read');
  assert.equal(await status(make(editor, { kind: 'workspace' })), 403, 'workspace tokens are owner-only');
  assert.equal((await make(owner, { kind: 'workspace', scopes: ['read', 'run'] })).token.slice(0, 12), 'spectra_wat_');

  for (const [fields, why] of [
    [{ name: '   ' }, 'empty name'],
    [{ name: 42 }, 'name not text'],
    [{ scopes: [] }, 'no scopes'],
    [{ scopes: ['admin'] }, 'unknown scope'],
    [{ scopes: 'read' }, 'scopes not a list'],
    [{ expiresInDays: 0 }, 'expires today'],
    [{ expiresInDays: 366 }, 'over a year'],
    [{ expiresInDays: 1.5 }, 'fractional days'],
    [{ kind: 'admin' }, 'unknown kind'],
  ]) assert.equal(await status(make(owner, fields)), 400, why);
  assert.equal((await make(owner, { name: 'x'.repeat(200) })).info.name.length, 60, 'long names are cut');
  await assert.rejects(sql.query(`UPDATE api_tokens SET scopes = ARRAY['run'] WHERE workspace_id = $1`, [team.id]), /check/i, 'the database refuses run without read');
  await assert.rejects(sql.query(`UPDATE api_tokens SET expires_at = created_at + interval '400 days' WHERE workspace_id = $1`, [team.id]), /check/i, 'and anything over a year');

  // At most 10 live personal tokens per person per workspace.
  const solo = await store.createUser('tok-many@x.com', 'h', 'M');
  const ws = await membership(solo.id, (await authFor(solo)).workspace.id);
  for (let i = 0; i < 10; i++) await t.createToken({ by: solo, workspace: ws, name: `t${i}`, scopes: ['read'] });
  assert.equal(await status(t.createToken({ by: solo, workspace: ws, name: 'one too many', scopes: ['read'] })), 409);
});

test('api tokens: personal tokens follow their owner (role changes, removal, disabling, new password)', async () => {
  const t = tok();
  const owner = await store.createUser('tok-p-owner@x.com', 'h', 'O');
  const member = await store.createUser('tok-p-member@x.com', 'h', 'M');
  const team = await workspaces.createWorkspace(owner.id, 'Personal Desk');
  await addMember(team.id, member.id, 'editor');
  const { token, info } = await t.createToken({ by: member, workspace: await membership(member.id, team.id), name: 'job', scopes: ['read', 'run'] });
  assert.equal((await t.authenticateToken(token)).workspace.role, 'editor');

  await sql.query(`UPDATE workspace_members SET role = 'viewer' WHERE workspace_id = $1 AND user_id = $2`, [team.id, member.id]);
  assert.equal((await t.authenticateToken(token)).workspace.role, 'viewer', 'demoted: the token can no longer run');
  await sql.query(`UPDATE workspace_members SET role = 'owner' WHERE workspace_id = $1 AND user_id = $2`, [team.id, member.id]);
  assert.equal((await t.authenticateToken(token)).workspace.role, 'editor', 'promoted: still capped by the token\'s scopes, never owner');

  await sql.query(`UPDATE users SET status = 'disabled' WHERE id = $1`, [member.id]);
  assert.equal(await t.authenticateToken(token), null, 'disabled account');
  await sql.query(`UPDATE users SET status = 'active' WHERE id = $1`, [member.id]);
  assert.ok(await t.authenticateToken(token));

  await sql.query(`DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2`, [team.id, member.id]);
  assert.equal(await t.authenticateToken(token), null, 'removed from the workspace (even without the team helper)');
  await addMember(team.id, member.id, 'editor');
  assert.ok(await t.authenticateToken(token));
  await teamLib().removeMember(team.id, member.id);
  assert.ok((await sql.query('SELECT revoked_at FROM api_tokens WHERE id = $1', [info.id])).rows[0].revoked_at, 'removeMember revokes their tokens there');
  await addMember(team.id, member.id, 'editor');
  assert.equal(await t.authenticateToken(token), null, 'and rejoining doesn\'t bring them back');

  const again = await t.createToken({ by: member, workspace: await membership(member.id, team.id), name: 'job 2', scopes: ['read'] });
  await store.updatePassword('tok-p-member@x.com', 'new-hash');
  assert.equal(await t.authenticateToken(again.token), null, 'a new password revokes personal tokens');

  const third = await t.createToken({ by: member, workspace: await membership(member.id, team.id), name: 'job 3', scopes: ['read'] });
  await sql.query('DELETE FROM workspace_members WHERE user_id = $1', [member.id]);
  await store.deleteUser('tok-p-member@x.com');
  assert.equal((await sql.query('SELECT count(*)::int AS n FROM api_tokens WHERE id = $1', [third.info.id])).rows[0].n, 0, 'deleting the account deletes its tokens');
});

test('api tokens: workspace tokens belong to the workspace; expiry, revocation and AUTH_SECRET', async () => {
  const t = tok();
  const owner = await store.createUser('tok-w-owner@x.com', 'h', 'O');
  const owner2 = await store.createUser('tok-w-owner2@x.com', 'h', 'O2');
  const team = await workspaces.createWorkspace(owner.id, 'Shared Desk');
  await addMember(team.id, owner2.id, 'owner');
  const { token, info } = await t.createToken({ by: owner, workspace: await membership(owner.id, team.id), kind: 'workspace', name: 'nightly', scopes: ['read', 'run'] });
  let auth = await t.authenticateToken(token);
  assert.equal(auth.user, null, 'acts as itself, not its creator');
  assert.equal(auth.workspace.role, 'editor');
  assert.equal(info.createdBy, 'tok-w-owner@x.com');

  await teamLib().removeMember(team.id, owner.id);
  assert.ok(await t.authenticateToken(token), 'keeps working when its creator leaves');

  // Expired.
  await sql.query(`UPDATE api_tokens SET created_at = now() - interval '2 days', expires_at = now() - interval '1 second' WHERE id = $1`, [info.id]);
  assert.equal(await t.authenticateToken(token), null, 'expired');
  await sql.query(`UPDATE api_tokens SET expires_at = now() + interval '1 day' WHERE id = $1`, [info.id]);

  // A different AUTH_SECRET makes every token unusable.
  const secret = process.env.AUTH_SECRET;
  process.env.AUTH_SECRET = 'another-secret-' + crypto.randomBytes(16).toString('hex');
  try {
    assert.equal(await t.authenticateToken(token), null);
  } finally {
    process.env.AUTH_SECRET = secret;
  }
  assert.ok(await t.authenticateToken(token));

  // Revoke.
  const ws2 = await membership(owner2.id, team.id);
  assert.equal((await t.revokeToken(ws2, info.id, owner2)).revokedAt > 0, true);
  assert.equal(await t.authenticateToken(token), null, 'revoked: stops at once');
  assert.equal(await t.revokeToken(ws2, info.id, owner2), null, 'already revoked');

  // Deleting the workspace deletes its tokens.
  const w = await t.createToken({ by: owner2, workspace: ws2, kind: 'workspace', name: 'temp', scopes: ['read'] });
  await workspaces.deleteWorkspace(team.id);
  assert.equal(await t.authenticateToken(w.token), null);
  assert.equal((await sql.query('SELECT count(*)::int AS n FROM api_tokens WHERE workspace_id = $1', [team.id])).rows[0].n, 0);
});

test('api tokens: owners see and revoke every token; others only their own', async () => {
  const t = tok();
  const owner = await store.createUser('tok-l-owner@x.com', 'h', 'O');
  const a = await store.createUser('tok-l-a@x.com', 'h', 'A');
  const b = await store.createUser('tok-l-b@x.com', 'h', 'B');
  const team = await workspaces.createWorkspace(owner.id, 'List Desk');
  const other = await workspaces.createWorkspace(owner.id, 'Other Desk');
  await addMember(team.id, a.id, 'editor');
  await addMember(team.id, b.id, 'viewer');
  const wsOf = (u) => membership(u.id, team.id);
  const ta = await t.createToken({ by: a, workspace: await wsOf(a), name: 'a', scopes: ['read'] });
  const tb = await t.createToken({ by: b, workspace: await wsOf(b), name: 'b', scopes: ['read'] });
  const tw = await t.createToken({ by: owner, workspace: await wsOf(owner), kind: 'workspace', name: 'w', scopes: ['read'] });
  await t.createToken({ by: owner, workspace: await membership(owner.id, other.id), name: 'elsewhere', scopes: ['read'] });

  assert.deepEqual((await t.listTokens(await wsOf(owner), owner.id)).map((x) => x.name).sort(), ['a', 'b', 'w']);
  assert.deepEqual((await t.listTokens(await wsOf(a), a.id)).map((x) => x.name), ['a']);
  assert.deepEqual((await t.listTokens(await wsOf(b), b.id)).map((x) => x.name), ['b']);

  assert.equal(await t.revokeToken(await wsOf(a), tb.info.id, a), null, 'can\'t revoke someone else\'s');
  assert.equal(await t.revokeToken(await wsOf(a), tw.info.id, a), null, 'or the workspace\'s');
  assert.equal(await t.revokeToken(await membership(owner.id, other.id), ta.info.id, owner), null, 'or one from another workspace');
  assert.ok(await t.revokeToken(await wsOf(a), ta.info.id, a), 'your own: yes');
  assert.ok(await t.revokeToken(await wsOf(owner), tb.info.id, owner), 'owners: any');
  assert.deepEqual((await t.listTokens(await wsOf(owner), owner.id)).map((x) => x.name), ['w'], 'revoked tokens drop off the list');
  assert.ok(!JSON.stringify(await t.listTokens(await wsOf(owner), owner.id)).includes('token_hash'));
});

test('api tokens: the engine proxy checks scope and role, logs every use, never sends admin rights', () => {
  const t = tok();
  assert.equal(t.scopeFor('viewer'), 'read');
  assert.equal(t.scopeFor('editor'), 'run');
  assert.equal(t.scopeFor('owner'), null, 'nothing owner-level for tokens');
  assert.equal(t.scopeRole(['read']), 'viewer');
  assert.equal(t.scopeRole(['read', 'run']), 'editor');

  const src = fs.readFileSync(path.join(root, 'app/api/engine/[...path]/route.ts'), 'utf8');
  const tokenBranch = src.slice(src.indexOf('bearer !== undefined'), src.indexOf('} else if (!route.public)'));
  assert.ok(tokenBranch.length > 200, 'a bearer header takes the token branch (never the cookie)');
  assert.match(tokenBranch, /authenticateToken\(bearer, ip\)/);
  assert.match(tokenBranch, /!caller\.token\.scopes\.includes\(scope\) \|\| !hasRole\(caller\.workspace\.role, minRole\)/);
  assert.equal(tokenBranch.match(/action: 'api_token\.used'/g)?.length, 1);
  assert.match(tokenBranch, /outcome: 'denied'/, 'refusals are logged');
  assert.ok(tokenBranch.indexOf("await audit({ ...used, details") < tokenBranch.indexOf('signEngineIdentity'), 'every allowed use is logged before it is forwarded');
  assert.match(tokenBranch, /platformRole: 'user'/);
  assert.match(tokenBranch, /hit\(`api-token:ip:\$\{ip\}`/, 'bad tokens are rate-limited per address');

  // Leaked tokens are caught by the secret scan.
  const gl = fs.readFileSync(path.join(root, '../../.gitleaks.toml'), 'utf8');
  const rule = /id = "spectra-api-token"[\s\S]*?regex = '''(.+?)'''/.exec(gl);
  assert.ok(rule, '.gitleaks.toml has a rule for Spectra API tokens');
  const re = new RegExp(rule[1].replace(/^\(\?i\)/, ''));
  assert.ok(re.test('spectra_pat_' + crypto.randomBytes(32).toString('base64url')));
  assert.ok(re.test('spectra_wat_' + crypto.randomBytes(32).toString('base64url')));
});

test('audit: tampering outside the app is detected (runs last: it breaks the chain on purpose)', async () => {
  assert.equal((await auditLog.verifyChain()).ok, true);
  const [{ seq }] = (await sql.query('SELECT seq FROM audit_events ORDER BY seq LIMIT 1 OFFSET 2')).rows;
  await sql.query('ALTER TABLE audit_events DISABLE TRIGGER audit_events_no_update');
  await sql.query(`UPDATE audit_events SET details = '{"forged": true}' WHERE seq = $1`, [seq]);
  await sql.query('ALTER TABLE audit_events ENABLE TRIGGER audit_events_no_update');
  let chain = await auditLog.verifyChain();
  assert.equal(chain.ok, false);
  assert.deepEqual(chain.breaks[0], { seq: String(seq), problem: 'row content altered' });

  const { execFileSync } = await import('node:child_process');
  assert.throws(
    () => execFileSync(process.execPath, [path.join(root, 'scripts/audit-verify.mjs')], { stdio: 'pipe', env: { ...process.env, DATABASE_URL: testUrl } }),
    (e) => e.status === 1 && /BROKEN/.test(String(e.stderr)),
    'npm run audit:verify fails loudly',
  );

  await sql.query('ALTER TABLE audit_events DISABLE TRIGGER audit_events_no_delete');
  await sql.query('DELETE FROM audit_events WHERE seq = $1', [Number(seq) + 5]);
  await sql.query('ALTER TABLE audit_events ENABLE TRIGGER audit_events_no_delete');
  chain = await auditLog.verifyChain();
  assert.ok(chain.breaks.some((b) => b.seq === String(Number(seq) + 6) && b.problem === 'previous row missing or altered'), 'a deleted row breaks the link after it');
});

test('toQrSvg generates deterministic SVG from otpauth URI and rejects invalid length', async () => {
  const uri = 'otpauth://totp/Spectra%20Labs:alice%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=Spectra%20Labs&algorithm=SHA1&digits=6&period=30';
  const svg1 = await qr.toQrSvg(uri);
  assert.ok(svg1.startsWith('<svg'), 'starts with <svg');
  assert.ok(svg1.includes('<path'), 'contains <path');

  const svg2 = await qr.toQrSvg(uri);
  assert.equal(svg1, svg2, 'same input gives the same output twice');

  await assert.rejects(async () => qr.toQrSvg(''), Error);
  await assert.rejects(async () => qr.toQrSvg('x'.repeat(1025)), Error);
});


test('uploads: spooled to a temp file while hashed, refused past the limit, names cleaned', async () => {
  const data = Buffer.from('Date,Close\n2024-01-02,471.5\n');
  const stream = (chunks) => new ReadableStream({ start(c) { for (const x of chunks) c.enqueue(x); c.close(); } });
  const s = await engineUpload.spoolToTempFile(stream([data.subarray(0, 5), data.subarray(5)]));
  try {
    assert.equal(s.size, data.length);
    assert.equal(s.sha256, crypto.createHash('sha256').update(data).digest('hex'));
    assert.deepEqual(fs.readFileSync(s.file), data);
    assert.equal(fs.statSync(s.file).mode & 0o077, 0, 'temp file is private');
  } finally {
    fs.rmSync(s.file, { force: true });
  }
  const tmpBefore = fs.readdirSync(os.tmpdir()).filter((f) => f.startsWith('spectra-upload-')).length;
  await assert.rejects(engineUpload.spoolToTempFile(stream([data, data]), data.length + 1), engineUpload.UploadTooLarge);
  assert.equal(fs.readdirSync(os.tmpdir()).filter((f) => f.startsWith('spectra-upload-')).length, tmpBefore, 'nothing left behind');
  assert.equal(engineUpload.MAX_UPLOAD_BYTES, 100 * 1024 * 1024);
  assert.equal(engineUpload.cleanFilename('C:\\Users\\me\\prices.csv'), 'prices.csv');
  assert.equal(engineUpload.cleanFilename('../../etc/passwd'), 'passwd');
  assert.equal(engineUpload.cleanFilename(' spy\u0000daily.csv '), 'spydaily.csv');
  assert.equal(engineUpload.cleanFilename(null), '');
  assert.equal(engineUpload.cleanFilename('x'.repeat(400)).length, 255);
});

test('uploads skip the middleware (it cuts bodies at 10 MB), and only that route does', () => {
  const text = fs.readFileSync(path.join(root, 'middleware.ts'), 'utf8');
  // The matcher is a single-quoted string literal; as JSON it reads the same.
  const literal = /matcher: \['((?:[^'\\]|\\.)+)'\]/.exec(text)[1];
  const re = new RegExp(`^${JSON.parse(`"${literal}"`)}$`);
  assert.ok(!re.test('/api/engine-upload'), 'upload route skipped');
  for (const p of ['/api/engine/api/v1/datasets', '/api/engine-upload/x', '/api/engine-uploads', '/studio', '/api/auth/login']) {
    assert.ok(re.test(p), `${p} still goes through the middleware`);
  }
});

test('data tab: settings come only from what the person chose; suggestions apply on request', async () => {
  const d = datasetsClient.emptyDraft();
  const empty = datasetsClient.settingsFromDraft(d);
  // Nothing is filled in: the engine reports every missing choice.
  assert.equal(empty.delimiter, '');
  assert.equal(empty.time_format, '');
  assert.equal(empty.utc_offset_minutes, null);
  assert.equal(empty.columns.close, '');
  assert.equal(empty.columns.volume, null, 'an optional column left empty means "none"');
  assert.equal(empty.skip_bad_rows, false, 'bad rows abort by default');
  const suggestions = {
    delimiter: ';',
    header: ['Date', 'Ticker', 'Open', 'High', 'Low', 'Close'],
    columns: { timestamp: 'Date', open: 'Open', high: 'High', low: 'Low', close: 'Close', volume: null, symbol: 'Ticker' },
    time_format: 'iso8601',
    timestamps_have_zone: false,
  };
  const applied = datasetsClient.applySuggestions(d, suggestions);
  assert.equal(applied.utc_offset_minutes, null, 'the offset is never suggested');
  const s = datasetsClient.settingsFromDraft({ ...applied, utc_offset_minutes: 330, default_symbol: 'IGNORED' });
  assert.deepEqual(
    { delimiter: s.delimiter, time_format: s.time_format, utc: s.utc_offset_minutes, symbol: s.columns.symbol, volume: s.columns.volume, default_symbol: s.default_symbol },
    { delimiter: ';', time_format: 'iso8601', utc: 330, symbol: 'Ticker', volume: null, default_symbol: null },
    'a symbol column means no default symbol',
  );
  const single = datasetsClient.settingsFromDraft({ ...applied, columns: { ...applied.columns, symbol: '' }, default_symbol: 'NIFTY' });
  assert.equal(single.default_symbol, 'NIFTY');
  assert.equal(datasetsClient.formatOffset(330), 'UTC+05:30');
  assert.equal(datasetsClient.formatOffset(-300), 'UTC−05:00');
  assert.equal(datasetsClient.formatOffset(0), 'UTC+00:00');

  // The preview sample: the first 1 MB, flagged when cut.
  const small = await datasetsClient.readSample(new Blob(['a,b\n1,2\n']));
  assert.deepEqual(small, { text: 'a,b\n1,2\n', truncated: false });
  const big = await datasetsClient.readSample(new Blob(['x'.repeat(datasetsClient.PREVIEW_SAMPLE_BYTES + 10)]));
  assert.equal(big.text.length, datasetsClient.PREVIEW_SAMPLE_BYTES);
  assert.ok(big.truncated);

  // Field errors from a 422's details, joined per field.
  const err = new apiClient.ApiError('bad', 422, { errors: [{ field: 'settings.columns.close', message: 'required' }, { field: 'dataset_name', message: 'required' }, { field: 'dataset_name', message: 'too long' }, { message: 'whole request' }] });
  assert.deepEqual(apiClient.detailErrors(err), { 'settings.columns.close': 'required', dataset_name: 'required; too long', '': 'whole request' });
  assert.deepEqual(apiClient.detailErrors(new Error('x')), {});
  assert.deepEqual(apiClient.detailErrors(new apiClient.ApiError('x', 500)), {});
  const fromResponse = await apiClient.errorFrom(new Response(JSON.stringify({ code: 'in_use', message: 'kept', details: null }), { status: 409 }));
  assert.equal(fromResponse.code, 'in_use');
  assert.equal(fromResponse.status, 409);
  assert.equal(fromResponse.message, 'kept');
});

test('engine request signing matches the Rust verifier', () => {
  const secret = 'test-secret-test-secret-test-secret!';
  const body = '{"metric":"var","note":"ünïcødé"}';
  const h = engineSign.signEngineRequest(
    secret,
    { method: 'post', pathAndQuery: '/api/v1/risk', body, requestId: '6f1c0e5e-1111-4222-8333-444455556666' },
    { user: 'alice@example.com' },
    1700000000 * 1000 + 999,
  );
  assert.equal(h['X-Spectra-Timestamp'], '1700000000');
  assert.equal(h['X-Spectra-Workspace'], '');
  // Same vector as the cross-check against crates/spectra-api/src/auth.rs.
  assert.equal(h['X-Spectra-Signature'], 'd218123bb0b8ba8ada3e81710dbe967c4ac2909055a55063b60ae03815ddd989');
  // Pinned canonical string, identical to the_canonical_string_is_stable in auth.rs.
  assert.equal(
    engineSign.canonical('post', '/api/v1/risk?x=1', '1700000000', 'u', '', 'r', ''),
    'spectra-v1\nPOST\n/api/v1/risk?x=1\n1700000000\nu\n\nr\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
  );
  // Uploads sign a body hash they computed while streaming: the same signature as signing the body.
  const hashed = engineSign.signEngineRequest(
    secret,
    {
      method: 'post',
      pathAndQuery: '/api/v1/risk',
      requestId: '6f1c0e5e-1111-4222-8333-444455556666',
      bodySha256: crypto.createHash('sha256').update(body, 'utf8').digest('hex'),
    },
    { user: 'alice@example.com' },
    1700000000 * 1000 + 999,
  );
  assert.equal(hashed['X-Spectra-Signature'], h['X-Spectra-Signature']);
  const before = process.env.ENGINE_SECRET;
  process.env.ENGINE_SECRET = 'too-short';
  assert.equal(engineSign.engineSecret(), null, 'short secrets are refused');
  process.env.ENGINE_SECRET = secret;
  assert.equal(engineSign.engineSecret(), secret);
  if (before === undefined) delete process.env.ENGINE_SECRET;
  else process.env.ENGINE_SECRET = before;
});
