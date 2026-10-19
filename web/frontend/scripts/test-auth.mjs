// Auth & security tests. Run: npm test
// Uses only Node's built-in test runner + the project's TypeScript compiler (no new deps).
// Runs inside a throwaway temp dir, so the real .data/users.json is never touched.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);
const ts = require(path.join(root, 'node_modules/typescript'));

const work = fs.mkdtempSync(path.join(os.tmpdir(), 'spectra-auth-test-'));
process.chdir(work); // authStore writes .data/ under cwd
process.env.AUTH_SECRET = 'test-secret-' + crypto.randomBytes(16).toString('hex');

function load(rel) {
  const src = fs.readFileSync(path.join(root, rel), 'utf8');
  const { outputText } = ts.transpileModule(src, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, esModuleInterop: true },
  });
  const out = path.join(work, rel.replace(/[\\/]/g, '__').replace(/\.tsx?$/, '.cjs'));
  fs.writeFileSync(out, outputText);
  return require(out);
}

const shared = load('lib/auth-shared.ts');
const store = load('lib/server/authStore.ts');
const edge = load('lib/sessionEdge.ts');
const rl = load('lib/server/rateLimit.ts');

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
});

test('sign-up codes are hashed at rest, single-use, and lock after 5 misses', () => {
  store.setPendingOtp('new@x.com', '123456', store.hashPassword('Passw0rd!'), 'New User');
  const onDisk = fs.readFileSync(path.join(work, '.data/pending-codes.json'), 'utf8');
  assert.ok(!onDisk.includes('123456'), 'plaintext code must not be written to disk');

  assert.equal(store.checkSignupCode('new@x.com', '000000').result, 'wrong');
  const ok = store.checkSignupCode('new@x.com', '123456');
  assert.equal(ok.result, 'ok');
  assert.equal(ok.entry.name, 'New User');
  assert.equal(store.checkSignupCode('new@x.com', '123456').result, 'missing', 'code is single-use');

  store.setPendingOtp('lock@x.com', '111111', 'h', '');
  for (let i = 0; i < 5; i++) assert.equal(store.checkSignupCode('lock@x.com', '999999').result, 'wrong');
  assert.equal(store.checkSignupCode('lock@x.com', '111111').result, 'locked', 'right code after 5 misses is refused');
});

test('sign-up and reset codes cannot be swapped', () => {
  store.setResetCode('swap@x.com', '222222');
  assert.equal(store.checkSignupCode('swap@x.com', '222222').result, 'missing');
  assert.equal(store.checkResetCode('swap@x.com', '222222').result, 'ok');
});

test('sessions: round-trip, tamper, unknown user', () => {
  const u = store.createUser('amogh@x.com', store.hashPassword('Passw0rd!'), 'Amogh');
  const tok = store.createSessionToken(store.publicUser(u));
  assert.deepEqual(store.verifySessionToken(tok), { email: 'amogh@x.com', role: 'user', name: 'Amogh' });
  const [p, sig] = tok.split('.');
  const forged = Buffer.from(JSON.stringify({ email: 'amogh@x.com', role: 'admin', iat: 9e9, exp: 9e9 })).toString('base64url') + '.' + sig;
  assert.equal(store.verifySessionToken(forged), null);
  assert.equal(store.verifySessionToken(store.createSessionToken({ email: 'ghost@x.com', role: 'user' })), null, 'no account → no session');
});

test('password reset revokes older sessions', async () => {
  const u = store.createUser('reset@x.com', store.hashPassword('OldPassw0rd!'), 'R');
  const old = store.createSessionToken(store.publicUser(u));
  await new Promise((r) => setTimeout(r, 1100)); // tokens have 1-second iat resolution
  store.updatePassword('reset@x.com', store.hashPassword('NewPassw0rd!'));
  assert.equal(store.verifySessionToken(old), null, 'pre-reset session must die');
  const fresh = store.createSessionToken(store.publicUser(store.findUser('reset@x.com')));
  assert.ok(store.verifySessionToken(fresh), 'post-reset session works');
  assert.ok(store.verifyPassword('NewPassw0rd!', store.findUser('reset@x.com').passwordHash));
  assert.ok(!store.verifyPassword('OldPassw0rd!', store.findUser('reset@x.com').passwordHash));
});

test('users file: atomic, owner-only, no hashes in admin listing', () => {
  const file = path.join(work, '.data/users.json');
  JSON.parse(fs.readFileSync(file, 'utf8')); // valid JSON
  if (process.platform !== 'win32') assert.equal(fs.statSync(file).mode & 0o077, 0, 'users.json must not be group/world readable');
  assert.deepEqual(fs.readdirSync(path.join(work, '.data')).filter((f) => f.endsWith('.tmp')), []);
  assert.ok(store.listUsers().every((x) => !('passwordHash' in x)));
});

test('middleware (Edge) verifier matches the server token format', async () => {
  const u = store.createUser('edge@x.com', 'h', 'E');
  const tok = store.createSessionToken(store.publicUser(u));
  assert.deepEqual(await edge.verifySessionEdge(tok), { email: 'edge@x.com', role: 'user' });
  assert.equal(await edge.verifySessionEdge(tok.slice(0, -2) + 'xx'), null);
  assert.equal(await edge.verifySessionEdge(undefined), null);
});

test('2FA: per-account setting, admin always on, login codes isolated', () => {
  const on = store.createUser('twofa@x.com', 'h', 'T'); // default for new accounts
  assert.equal(store.twoFactorOn(on), true);
  const off = store.createUser('onefa@x.com', 'h', 'O', false);
  assert.equal(store.twoFactorOn(off), false);
  store.setTwoFactor('onefa@x.com', true);
  assert.equal(store.twoFactorOn(store.findUser('onefa@x.com')), true);

  const admin = store.createUser(store.ADMIN_EMAIL, 'h', 'Admin', false); // asks for off...
  assert.equal(store.twoFactorOn(admin), true, '...but admins are always 2FA');
  store.setTwoFactor(store.ADMIN_EMAIL, false);
  assert.equal(store.twoFactorOn(store.findUser(store.ADMIN_EMAIL)), true, 'admin cannot switch it off');

  store.setLoginCode('twofa@x.com', '333333');
  assert.equal(store.checkSignupCode('twofa@x.com', '333333').result, 'missing', 'login code is not a sign-up code');
  assert.equal(store.checkResetCode('twofa@x.com', '333333').result, 'missing', 'login code is not a reset code');
  assert.equal(store.checkLoginCode('twofa@x.com', '333333').result, 'ok');
  assert.equal(store.checkLoginCode('twofa@x.com', '333333').result, 'missing', 'single-use');
  assert.equal(store.listUsers().find((u) => u.email === 'twofa@x.com').twoFactor, true);
});

test('account: rename, sign out everywhere, delete', async () => {
  const u = store.createUser('acct@x.com', store.hashPassword('Passw0rd!'), 'Old Name');
  const before = store.createSessionToken(store.publicUser(u));
  store.updateName('acct@x.com', 'New Name');
  assert.equal(store.verifySessionToken(before).name, 'New Name', 'name is read live, so the header updates without re-login');

  await new Promise((r) => setTimeout(r, 1100)); // tokens have 1-second iat resolution
  store.revokeSessions('acct@x.com');
  assert.equal(store.verifySessionToken(before), null, 'older sessions die');
  const current = store.createSessionToken(store.publicUser(store.findUser('acct@x.com')));
  assert.ok(store.verifySessionToken(current), 're-issued session for this browser works');

  store.setLoginCode('acct@x.com', '444444');
  assert.equal(store.deleteUser('acct@x.com'), true);
  assert.equal(store.findUser('acct@x.com'), undefined);
  assert.equal(store.verifySessionToken(current), null, 'deleted account → no session');
  assert.equal(store.checkLoginCode('acct@x.com', '444444').result, 'missing', 'pending codes are removed too');
  assert.ok(!store.listUsers().some((x) => x.email === 'acct@x.com'));
});

test('AUTH_SECRET: no fallback; missing, short or retired secrets fail closed', async () => {
  const good = process.env.AUTH_SECRET;
  const token = store.createSessionToken({ email: 'nobody@x.com', role: 'user' });
  try {
    for (const bad of [undefined, '', 'short-secret', 'spectralabs_jwt_default_secret_key_32bytes_required']) {
      if (bad === undefined) delete process.env.AUTH_SECRET;
      else process.env.AUTH_SECRET = bad;
      assert.throws(() => store.createSessionToken({ email: 'a@b.c', role: 'user' }), /AUTH_SECRET/, `server refuses to sign with ${JSON.stringify(bad)}`);
      assert.equal(store.verifySessionToken(token), null, 'server verifier rejects');
      assert.equal(await edge.verifySessionEdge(token), null, 'edge verifier rejects');
    }
  } finally {
    process.env.AUTH_SECRET = good;
  }
});

test('setup script generates a strong AUTH_SECRET and keeps a good one', async () => {
  const { execFileSync } = await import('node:child_process');
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'spectra-setup-test-'));
  fs.mkdirSync(path.join(dir, 'scripts'));
  fs.copyFileSync(path.join(root, 'scripts/setup-env.mjs'), path.join(dir, 'scripts/setup-env.mjs'));
  fs.copyFileSync(path.join(root, '.env.example'), path.join(dir, '.env.example'));
  const run = () => execFileSync(process.execPath, [path.join(dir, 'scripts/setup-env.mjs')], { encoding: 'utf8' });
  const secretIn = () => fs.readFileSync(path.join(dir, '.env.local'), 'utf8').match(/^AUTH_SECRET=(.*)$/m)[1];
  try {
    const out = run();
    const first = secretIn();
    assert.ok(first.length >= 60, 'long random secret');
    assert.ok(!out.includes(first), 'secret is never printed');
    run();
    assert.equal(secretIn(), first, 'a good secret is left alone');
    const env = path.join(dir, '.env.local');
    fs.writeFileSync(env, fs.readFileSync(env, 'utf8').replace(/^AUTH_SECRET=.*$/m, 'AUTH_SECRET=spectralabs_jwt_default_secret_key_32bytes_required'));
    run();
    assert.notEqual(secretIn(), 'spectralabs_jwt_default_secret_key_32bytes_required', 'the retired default is replaced');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('rate limiter', () => {
  for (let i = 0; i < 3; i++) assert.ok(rl.hit('t', 3, 60_000).ok);
  const r = rl.hit('t', 3, 60_000);
  assert.ok(!r.ok && r.retryAfter > 0 && r.retryAfter <= 60);
  assert.ok(!rl.peek('t', 3).ok);
  rl.reset('t');
  assert.ok(rl.peek('t', 3).ok);
});

process.on('exit', () => fs.rmSync(work, { recursive: true, force: true }));
