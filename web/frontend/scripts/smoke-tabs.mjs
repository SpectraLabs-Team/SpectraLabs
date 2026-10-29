// Smoke test: opens every Studio tab in a real browser (light and dark theme) and fails on
// crashes, error overlays, uncaught errors and failed API calls (HTTP 4xx/5xx).
//
// First time (opens a visible browser; log in normally, including the email code / 2FA):
//   npm run smoke:login
// Then, with `npm run dev` running in another terminal:
//   npm run smoke
//
// Options (env): SMOKE_BASE_URL (default http://localhost:3000), SMOKE_TABS=risk,paper (subset),
// SMOKE_THEMES=light (one theme), SMOKE_HEADED=1 (watch it run).
// Screenshots land in smoke-report/<tab>-<theme>.png, handy for spotting light/dark layout issues.
// The tab list is read from components/studio/StudioShell.tsx, so new tabs are covered automatically.

import { readFileSync, mkdirSync, existsSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

let chromium;
try {
  ({ chromium } = await import('playwright'));
} catch {
  console.error('Playwright is not installed. Run:\n  npm i -D playwright && npx playwright install chromium');
  process.exit(2);
}

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const BASE = (process.env.SMOKE_BASE_URL ?? 'http://localhost:3000').replace(/\/$/, '');
const AUTH = join(root, '.smoke-auth.json');
const OUT = join(root, 'smoke-report');
const THEMES = (process.env.SMOKE_THEMES ?? 'light,dark').split(',').map((t) => t.trim()).filter(Boolean);
const SETTLE_MS = 2500;

// ---- Login mode: save a session for later headless runs ---------------------------------
if (process.argv.includes('--login')) {
  const browser = await chromium.launch({ headless: false });
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.goto(BASE);
  console.log(`Log in in the browser window that opened (${BASE}). Waiting for /studio (up to 5 minutes)...`);
  await page.waitForURL(/\/studio/, { timeout: 5 * 60_000 });
  await ctx.storageState({ path: AUTH });
  console.log(`Saved session to ${AUTH} (git-ignored). Now run: npm run smoke`);
  await browser.close();
  process.exit(0);
}

if (!existsSync(AUTH)) {
  console.error('No saved session. Run `npm run smoke:login` first.');
  process.exit(2);
}

// ---- Tabs ---------------------------------------------------------------------------------
const shell = readFileSync(join(root, 'components/studio/StudioShell.tsx'), 'utf8');
const typeLine = shell.match(/export type StudioTab = ([^;]+);/);
let tabs = typeLine ? [...typeLine[1].matchAll(/'([a-z-]+)'/g)].map((m) => m[1]) : [];
if (!tabs.length) {
  console.error('Could not read the tab list from StudioShell.tsx.');
  process.exit(2);
}
if (process.env.SMOKE_TABS) {
  const want = new Set(process.env.SMOKE_TABS.split(',').map((t) => t.trim()));
  tabs = tabs.filter((t) => want.has(t));
}

// Noise that is not an app bug.
const IGNORE = [/favicon/i, /__nextjs/i, /webpack-hmr|turbopack-hmr|_next\/static/i, /Download the React DevTools/i];
const ignored = (s) => IGNORE.some((r) => r.test(s));

// ---- Run ------------------------------------------------------------------------------------
mkdirSync(OUT, { recursive: true });
const browser = await chromium.launch({ headless: !process.env.SMOKE_HEADED });
const problems = [];
const summary = [];

for (const theme of THEMES) {
  const ctx = await browser.newContext({ storageState: AUTH, viewport: { width: 1440, height: 900 } });
  await ctx.addInitScript((t) => {
    try { localStorage.setItem('spectra-theme', t); } catch {}
  }, theme);

  for (const tab of tabs) {
    const page = await ctx.newPage();
    const found = [];
    const add = (kind, detail) => { if (!ignored(detail)) found.push({ kind, detail: detail.slice(0, 300) }); };

    page.on('pageerror', (e) => add('uncaught error', e.message));
    page.on('console', (m) => { if (m.type() === 'error') add('console error', m.text()); });
    page.on('response', (r) => {
      const u = r.url();
      if (r.status() >= 400 && u.startsWith(BASE) && u.includes('/api/')) add(`HTTP ${r.status()}`, `${r.request().method()} ${u.slice(BASE.length)}`);
    });

    const url = `${BASE}/studio?tab=${tab}`;
    try {
      await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30_000 });
      if (!page.url().includes('/studio')) add('redirected', `${url} -> ${page.url()} (session expired? run npm run smoke:login)`);
      await page.waitForLoadState('networkidle', { timeout: 15_000 }).catch(() => {});
      await page.waitForTimeout(SETTLE_MS);

      // Click in-page tabs/segments (safe: they only switch views, never submit or delete).
      const inner = page.locator('main [role="tab"]');
      const n = Math.min(await inner.count(), 12);
      for (let i = 0; i < n; i++) {
        await inner.nth(i).click({ timeout: 2_000 }).catch(() => {});
        await page.waitForTimeout(400);
      }

      // Next.js error overlay / React error boundary.
      const overlay = await page.locator('nextjs-portal').count();
      const body = await page.locator('body').innerText().catch(() => '');
      if (overlay && /Runtime (Type)?Error|Unhandled Runtime Error|Build Error/i.test(await page.content())) add('error overlay', 'Next.js error overlay is showing');
      if (/Application error: a client-side exception|Something went wrong/i.test(body)) add('crash screen', body.match(/.{0,80}(Application error|Something went wrong).{0,80}/i)?.[0] ?? '');

      await page.screenshot({ path: join(OUT, `${tab}-${theme}.png`), fullPage: true }).catch(() => {});
    } catch (e) {
      add('navigation failed', String(e.message ?? e));
    }

    // De-duplicate repeats (polling endpoints fail the same way many times).
    const uniq = [...new Map(found.map((f) => [`${f.kind}|${f.detail}`, f])).values()];
    summary.push({ tab, theme, issues: uniq.length });
    for (const f of uniq) problems.push({ tab, theme, ...f });
    console.log(`${uniq.length ? '✗' : '✓'} ${tab.padEnd(13)} ${theme.padEnd(5)} ${uniq.length ? `${uniq.length} issue(s)` : 'ok'}`);
    await page.close();
  }
  await ctx.close();
}
await browser.close();

writeFileSync(join(OUT, 'report.json'), JSON.stringify({ base: BASE, at: new Date().toISOString(), summary, problems }, null, 2));
if (problems.length) {
  console.log(`\n${problems.length} problem(s):`);
  for (const p of problems) console.log(`  [${p.tab}/${p.theme}] ${p.kind}: ${p.detail}`);
  console.log(`\nScreenshots and report.json in ${OUT}`);
  process.exit(1);
}
console.log(`\nAll ${summary.length} tab/theme checks passed. Screenshots in ${OUT}`);
