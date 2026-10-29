// Role-gate tests for the admin console (M21 task 1). Unlike test-auth.mjs, which checks the
// route source, this calls the running app over HTTP and checks who is refused.
//
// Needs:
//   - the app running (npm run dev or npm start), at ADMIN_GATE_BASE_URL (default http://localhost:3000)
//   - the SAME DATABASE_URL and AUTH_SECRET as that app (read from the shell, then .env.local)
//   - for the page check: Playwright's Chromium (npx playwright install chromium); skipped if missing
//
// Run: npm run test:admin-gate
//
// It creates throwaway accounts named gate-<random>@example.test and deletes them at the end.

import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const ts = require('typescript');

// ── Environment: shell first, then .env.local (never overrides the shell) ──
const envFile = path.join(root, '.env.local');
if (fs.existsSync(envFile)) {
  for (const line of fs.readFileSync(envFile, 'utf8').split('\n')) {
    const m = line.match(/^\s*([A-Z0-9_]+)\s*=\s*(.*)\s*$/);
    if (m && process.env[m[1]] === undefined) process.env[m[1]] = m[2].replace(/^['"]|['"]$/g, '');
  }
}
const BASE = (process.env.ADMIN_GATE_BASE_URL ?? 'http://localhost:3000').replace(/\/$/, '');
for (const k of ['DATABASE_URL', 'AUTH_SECRET']) {
  if (!process.env[k]) {
    console.error(`\n  ✖ ${k} is not set. Use the same value as the running app (see web/README.md).\n`);
    process.exit(1);
  }
}
try {
  await fetch(`${BASE}/api/auth/session`);
} catch (err) {
  console.error(`\n  ✖ The app is not reachable at ${BASE} (${err.message}). Start it first: npm run dev\n`);
  process.exit(1);
}

// ── Load the server modules the same way test-auth.mjs does (its file list, kept in sync) ──
const authTest = fs.readFileSync(path.join(root, 'scripts/test-auth.mjs'), 'utf8');
const listMatch = authTest.match(/for \(const rel of (\[[^\]]+\])\)/);
if (!listMatch) throw new Error('Could not find the module list in test-auth.mjs; update test-admin-gate.mjs.');
const modules = JSON.parse(listMatch[1].replace(/'/g, '"'));
const build = path.join(root, 'node_modules', '.cache', 'spectra-admin-gate-test');
fs.rmSync(build, { recursive: true, force: true });
fs.mkdirSync(build, { recursive: true });
fs.writeFileSync(path.join(build, 'package.json'), '{"type":"commonjs"}');
for (const rel of modules) {
  const { outputText } = ts.transpileModule(fs.readFileSync(path.join(root, rel), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, esModuleInterop: true },
  });
  const out = path.join(build, rel.replace(/\.tsx?$/, '.js'));
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, outputText);
}
const load = (rel) => require(path.join(build, rel));
const { query } = load('lib/server/db.js');
const store = load('lib/server/authStore.js');
const sessions = load('lib/server/sessions.js');

// ── Tiny test runner ──
let passed = 0;
const failures = [];
const skipped = [];
async function test(name, fn) {
  try {
    await fn();
    passed++;
    console.log(`  ✔ ${name}`);
  } catch (err) {
    failures.push(name);
    console.log(`  ✖ ${name}\n      ${err.message}`);
  }
}
function expect(cond, msg) {
  if (!cond) throw new Error(msg);
}

// ── Throwaway accounts ──
const tag = crypto.randomBytes(4).toString('hex');
const created = [];
async function account(label, role) {
  const email = `gate-${label}-${tag}@example.test`;
  const user = await store.createUser(email, '!', `Gate ${label}`, true);
  created.push(email);
  if (role === 'admin') await query(`UPDATE users SET platform_role = 'admin' WHERE id = $1`, [user.id]);
  const { token, session } = await sessions.createSession(user, { userAgent: 'test-admin-gate' });
  return { user, token, session };
}

async function call(method, route, token, body) {
  const headers = { Origin: BASE };
  if (token) headers.Cookie = `spectra_session=${token}`;
  if (body) headers['Content-Type'] = 'application/json';
  const res = await fetch(`${BASE}${route}`, { method, headers, body: body ? JSON.stringify(body) : undefined, redirect: 'manual' });
  let json = null;
  try {
    json = await res.json();
  } catch {}
  return { status: res.status, json };
}

// ── Every admin route and method, read from the code (so new routes are covered automatically) ──
function adminRoutes() {
  const dir = path.join(root, 'app/api/admin');
  const found = [];
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.name === 'route.ts') {
        const src = fs.readFileSync(p, 'utf8');
        const rel = '/' + path.relative(path.join(root, 'app'), path.dirname(p)).split(path.sep).join('/');
        for (const m of src.matchAll(/export (?:async )?function (GET|POST|PUT|PATCH|DELETE)\b|export const (GET|POST|PUT|PATCH|DELETE)\b/g)) {
          found.push({ method: m[1] ?? m[2], pattern: rel });
        }
      }
    }
  };
  walk(dir);
  return found;
}
const fill = (pattern, id) => pattern.replace(/\[[^\]]+\]/g, id);
const bodyFor = (method) => (method === 'GET' || method === 'DELETE' ? undefined : { role: 'user' });

console.log(`\nAdmin role-gate tests against ${BASE}\n`);

try {
  const admin = await account('admin', 'admin');
  const admin2 = await account('admin2', 'admin');
  const user = await account('user', 'user');
  const target = await account('target', 'user');
  const routes = adminRoutes();

  await test(`found admin routes in the code (${routes.length})`, () => {
    expect(routes.length > 0, 'no routes found under app/api/admin');
  });

  // Sanity: an admin gets through. Without this, every 403 below could be a broken test setup.
  for (const r of routes.filter((r) => r.method === 'GET')) {
    const route = fill(r.pattern, target.user.id);
    await test(`admin is allowed: GET ${route.replace(target.user.id, '[id]')}`, async () => {
      const { status } = await call('GET', route, admin.token);
      expect(status === 200, `expected 200, got ${status}`);
    });
  }

  for (const r of routes) {
    const route = fill(r.pattern, target.user.id);
    const shown = `${r.method} ${r.pattern}`;
    await test(`anonymous is refused (401): ${shown}`, async () => {
      const { status, json } = await call(r.method, route, null, bodyFor(r.method));
      expect(status === 401, `expected 401, got ${status} ${JSON.stringify(json)}`);
    });
    await test(`signed-in non-admin is refused (403): ${shown}`, async () => {
      const { status, json } = await call(r.method, route, user.token, bodyFor(r.method));
      expect(status === 403, `expected 403, got ${status} ${JSON.stringify(json)}`);
      expect(json?.error === 'Admin privileges required.', `expected the admin-required error, got ${JSON.stringify(json)}`);
    });
  }

  await test('non-admin request did not change the target', async () => {
    const [row] = await query(`SELECT platform_role FROM users WHERE id = $1`, [target.user.id]);
    expect(row.platform_role === 'user', `target role is now ${row.platform_role}`);
  });

  await test('admin change without a recent confirm gets 403 with reauth', async () => {
    const stale = await account('stale', 'admin');
    await query(`UPDATE sessions SET reauth_at = now() - interval '1 hour' WHERE id = $1`, [stale.session.id]);
    const { status, json } = await call('PATCH', `/api/admin/users/${target.user.id}`, stale.token, { role: 'admin' });
    expect(status === 403 && json?.reauth === true, `expected 403 with reauth:true, got ${status} ${JSON.stringify(json)}`);
    const [row] = await query(`SELECT platform_role FROM users WHERE id = $1`, [target.user.id]);
    expect(row.platform_role === 'user', 'the change went through without a confirm');
  });

  await test('admin cannot change their own role through the web API', async () => {
    const { status } = await call('PATCH', `/api/admin/users/${admin.user.id}`, admin.token, { role: 'user' });
    expect(status === 403, `expected 403, got ${status}`);
  });

  await test('removing an admin role through the web API signs them out', async () => {
    const before = await call('GET', '/api/admin/overview', admin2.token);
    expect(before.status === 200, `admin2 should start with access, got ${before.status}`);
    const change = await call('PATCH', `/api/admin/users/${admin2.user.id}`, admin.token, { role: 'user' });
    expect(change.status === 200, `role change failed: ${change.status} ${JSON.stringify(change.json)}`);
    const after = await call('GET', '/api/admin/overview', admin2.token);
    expect(after.status === 401, `expected 401 (session revoked), got ${after.status}`);
  });

  // ── The /admin page: RequireAdmin shows an "Admin Privileges Required" screen (no redirect) ──
  let chromium = null;
  try {
    ({ chromium } = require('playwright'));
  } catch {}
  let browser = null;
  if (chromium) {
    try {
      browser = await chromium.launch();
    } catch (err) {
      skipped.push(`page check (Chromium not available: ${err.message.split('\n')[0]})`);
    }
  } else {
    skipped.push('page check (playwright not installed)');
  }
  if (browser) {
    const host = new URL(BASE).hostname;
    for (const [who, token] of [['anonymous', null], ['signed-in non-admin', user.token]]) {
      await test(`/admin shows the unauthorized screen to ${who}`, async () => {
        const context = await browser.newContext();
        if (token) await context.addCookies([{ name: 'spectra_session', value: token, domain: host, path: '/' }]);
        const page = await context.newPage();
        await page.goto(`${BASE}/admin`, { waitUntil: 'domcontentloaded' });
        const blocked = await page.getByText('Admin Privileges Required').first().isVisible({ timeout: 15000 }).catch(() => false);
        const onLogin = !page.url().includes('/admin');
        await context.close();
        expect(blocked || onLogin, `no unauthorized screen and still on ${page.url()}`);
      });
    }
    await test('/admin shows the dashboard to an admin (sanity)', async () => {
      const context = await browser.newContext();
      await context.addCookies([{ name: 'spectra_session', value: admin.token, domain: host, path: '/' }]);
      const page = await context.newPage();
      await page.goto(`${BASE}/admin`, { waitUntil: 'networkidle' });
      const blocked = await page.getByText('Admin Privileges Required').first().isVisible().catch(() => false);
      await context.close();
      expect(!blocked, 'an admin saw the unauthorized screen');
    });
    await browser.close();
  } else {
    // Weaker fallback: the page is wrapped in RequireAdmin.
    await test('/admin page is wrapped in RequireAdmin (source check, weaker)', () => {
      const src = fs.readFileSync(path.join(root, 'app/admin/page.tsx'), 'utf8');
      expect(/<RequireAdmin[\s>]/.test(src), 'app/admin/page.tsx does not use RequireAdmin');
    });
  }
} finally {
  for (const email of created) {
    try {
      await store.deleteUser(email);
    } catch (err) {
      console.log(`  ! could not delete ${email}: ${err.message}`);
    }
  }
  fs.rmSync(build, { recursive: true, force: true });
}

console.log(`\n${passed} passed, ${failures.length} failed${skipped.length ? `, skipped: ${skipped.join('; ')}` : ''}\n`);
process.exit(failures.length ? 1 : 0);
