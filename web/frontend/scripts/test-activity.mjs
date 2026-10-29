// Active-time accounting, the admin range and directory limits, and the browser's idle rule.
// No database needed: the functions that touch the database are checked for their input rules only.
// Run: node scripts/test-activity.mjs
import { describe, it, after } from 'node:test';
import assert from 'node:assert/strict';
import { transpileForTest } from './transpile-for-test.mjs';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');

const t = transpileForTest('activity', {
  files: ['lib/server/activity.ts', 'lib/server/db.ts', 'lib/server/adminAnalytics.ts', 'lib/activityClient.ts', 'lib/adminTypes.ts'],
});
after(t.cleanup);

const activity = t.load('lib/server/activity.js');
const analytics = t.load('lib/server/adminAnalytics.js');
const client = t.load('lib/activityClient.js');

describe('retention disclosure', () => {
  it('tells users the same retention period the purge job uses', () => {
    assert.equal(client.ACTIVITY_RETENTION_DAYS_CLIENT, activity.ACTIVITY_RETENTION_DAYS);
  });

  it('uses one retention period in the dashboard, the worker and the purge script', () => {
    const read = (rel) => fs.readFileSync(path.join(repoRoot, rel), 'utf8');
    const worker = read('crates/spectra-db/src/activity.rs').match(/pub const ACTIVITY_RETENTION_DAYS: i32 = (\d+);/);
    assert.ok(worker, 'ACTIVITY_RETENTION_DAYS not found in crates/spectra-db/src/activity.rs');
    assert.equal(Number(worker[1]), activity.ACTIVITY_RETENTION_DAYS, 'worker purge (Rust)');
    const script = read('web/frontend/scripts/activity-purge.mjs').match(/const DEFAULT_RETENTION_DAYS = (\d+);/);
    assert.ok(script, 'DEFAULT_RETENTION_DAYS not found in scripts/activity-purge.mjs');
    assert.equal(Number(script[1]), activity.ACTIVITY_RETENTION_DAYS, 'npm run activity:purge');
  });

  it('the worker and the script share one purge lock', () => {
    const lock = /'spectra:activity-purge'|"spectra:activity-purge"/;
    assert.match(fs.readFileSync(path.join(repoRoot, 'crates/spectra-db/src/activity.rs'), 'utf8'), lock);
    assert.match(fs.readFileSync(path.join(repoRoot, 'web/frontend/scripts/activity-purge.mjs'), 'utf8'), lock);
  });
});

describe('heartbeat credit', () => {
  it('credits nothing without a previous heartbeat, or for a gap that is zero, negative or not a number', () => {
    assert.equal(activity.heartbeatCredit(null), 0);
    assert.equal(activity.heartbeatCredit(0), 0);
    assert.equal(activity.heartbeatCredit(-5), 0);
    assert.equal(activity.heartbeatCredit(Number.NaN), 0);
  });

  it('credits the gap, rounded down, up to one minute', () => {
    assert.equal(activity.heartbeatCredit(30), 30);
    assert.equal(activity.heartbeatCredit(59.9), 59);
    assert.equal(activity.heartbeatCredit(60), 60);
    assert.equal(activity.heartbeatCredit(90), 60);
    assert.equal(activity.heartbeatCredit(120), 60);
  });

  it('credits nothing when the gap is longer than the limit (the page was closed, hidden or asleep)', () => {
    assert.equal(activity.heartbeatCredit(121), 0);
    assert.equal(activity.heartbeatCredit(3600), 0);
  });

  it('uses the UTC calendar day', () => {
    assert.equal(activity.activityDay(new Date('2026-10-27T23:59:59Z')), '2026-10-27');
    assert.equal(activity.activityDay(new Date('2026-10-28T00:00:00Z')), '2026-10-28');
  });

  it('refuses a retention below one day', async () => {
    await assert.rejects(() => activity.purgeActivityOlderThan(0), /at least 1/);
    await assert.rejects(() => activity.purgeActivityOlderThan(2.5), /whole number/);
  });
});

describe('admin date range', () => {
  const now = new Date('2026-10-27T12:00:00Z');

  it('defaults to the last 30 days, inclusive, ending today', () => {
    const r = analytics.parseRange(null, null, now);
    assert.equal(r.to, '2026-10-27');
    assert.equal(r.days, 30);
    assert.equal(r.from, '2026-09-28');
  });

  it('accepts a valid range and counts its days inclusively', () => {
    const r = analytics.parseRange('2026-10-01', '2026-10-07', now);
    assert.deepEqual(r, { from: '2026-10-01', to: '2026-10-07', days: 7 });
  });

  it('ignores malformed dates and falls back to the default range', () => {
    const r = analytics.parseRange('yesterday', '2026-02-31', now);
    assert.equal(r.to, '2026-10-27');
    assert.equal(r.days, 30);
  });

  it('moves a start after the end back to the end', () => {
    const r = analytics.parseRange('2026-10-10', '2026-10-01', now);
    assert.deepEqual(r, { from: '2026-10-01', to: '2026-10-01', days: 1 });
  });

  it('caps a range at one year', () => {
    const r = analytics.parseRange('2000-01-01', '2026-10-27', now);
    assert.equal(r.days, analytics.MAX_RANGE_DAYS);
    assert.equal(r.from, '2025-10-27');
  });
});

describe('browser activity rule', () => {
  const now = 1_000_000_000;
  it('counts a visible page used within the inactivity window', () => {
    assert.equal(client.isActive({ visible: true, lastInputAt: now - 1000, now }), true);
    assert.equal(client.isActive({ visible: true, lastInputAt: now - client.INACTIVITY_MS, now }), true);
  });

  it('does not count a hidden page, or one idle for longer than the window', () => {
    assert.equal(client.isActive({ visible: false, lastInputAt: now, now }), false);
    assert.equal(client.isActive({ visible: true, lastInputAt: now - client.INACTIVITY_MS - 1, now }), false);
  });

  it('does not count an input timestamp from the future (a clock change)', () => {
    assert.equal(client.isActive({ visible: true, lastInputAt: now + 5000, now }), false);
  });

  it('sends a heartbeat about once a minute, matching the server credit cap', () => {
    assert.equal(client.HEARTBEAT_MS, 60_000);
    assert.equal(client.HEARTBEAT_MS / 1000, activity.HEARTBEAT_INTERVAL_SECONDS);
  });
});
