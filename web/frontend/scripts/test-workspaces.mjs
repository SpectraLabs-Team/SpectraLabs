// Workspace guides, guided tour coverage and their links. No database or browser needed.
// Run: node scripts/test-workspaces.mjs
import { describe, it, after } from 'node:test';
import assert from 'node:assert/strict';
import { transpileForTest } from './transpile-for-test.mjs';

const t = transpileForTest('workspaces', {
  files: ['lib/workspaces/registry.ts', 'components/onboarding/tourSteps.ts', 'lib/onboarding.ts', 'lib/activityClient.ts'],
  dirs: ['lib/learn'],
});
after(t.cleanup);

const { WORKSPACES, workspacesFor } = t.load('lib/workspaces/registry.js');
const { MAIN_TOUR, PAGE_TOURS } = t.load('components/onboarding/tourSteps.js');
const { LESSONS, metricDefinitions } = (() => {
  const learn = t.load('lib/learn/index.js');
  return { LESSONS: learn.LESSONS, metricDefinitions: t.load('lib/learn/metrics.js').metricDefinitions };
})();

// Workspace ids as the Studio defines them. Read from the source so this test fails when a
// workspace is added to TABS without a guide and tour step.
const shellSource = t.source('components/studio/StudioShell.tsx');
const TAB_IDS = [...shellSource.matchAll(/\{ id: '([a-z-]+)', label:/g)].map((m) => m[1]);

const validLessonHrefs = new Set([
  ...LESSONS.map((l) => `/learn/${l.category}/${l.slug}`),
  ...Object.values(metricDefinitions).map((d) => d.learnPath),
  '/learn/metrics',
]);

describe('workspace guides', () => {
  it('finds the Studio workspaces in StudioShell', () => {
    assert.ok(TAB_IDS.length >= 10, `expected the Studio's workspaces, found ${TAB_IDS.length}`);
    assert.equal(new Set(TAB_IDS).size, TAB_IDS.length, 'duplicate workspace ids');
  });

  it('has exactly one guide per workspace, and no guide for an unknown workspace', () => {
    assert.deepEqual(Object.keys(WORKSPACES).sort(), [...TAB_IDS].sort());
  });

  it('gives every guide a purpose, steps, inputs, controls, outputs, pitfalls and at least one lesson', () => {
    for (const [id, g] of Object.entries(WORKSPACES)) {
      assert.ok(g.purpose.length > 40, `${id}: purpose too short`);
      assert.ok(g.steps.length >= 2, `${id}: needs at least two steps`);
      for (const field of ['inputs', 'controls', 'outputs', 'pitfalls']) {
        assert.ok(g[field].length >= 1, `${id}: ${field} is empty`);
      }
      assert.ok(g.lessons.length >= 1, `${id}: no lesson links`);
    }
  });

  it('links only to lessons and metric pages that exist', () => {
    for (const [id, g] of Object.entries(WORKSPACES)) {
      for (const l of g.lessons) {
        assert.ok(validLessonHrefs.has(l.href), `${id}: broken lesson link ${l.href}`);
      }
    }
  });

  it('gives each user a list of the workspaces they may open', () => {
    assert.ok(workspacesFor('user').every((id) => TAB_IDS.includes(id)));
    assert.ok(workspacesFor('admin').length >= workspacesFor('user').length);
  });
});

describe('guided tour coverage', () => {
  const navTargets = (step) => step.targets.filter((x) => x.startsWith('nav-')).map((x) => x.slice(4));

  it('has a main-tour step for every workspace', () => {
    const covered = new Set(MAIN_TOUR.flatMap(navTargets));
    const missing = TAB_IDS.filter((id) => !covered.has(id));
    assert.deepEqual(missing, [], 'workspaces with no tour step');
  });

  it('only names workspaces and features that exist', () => {
    const known = new Set([...TAB_IDS, 'learn']);
    for (const step of MAIN_TOUR) {
      for (const id of navTargets(step)) assert.ok(known.has(id), `step ${step.id} targets unknown nav-${id}`);
      if (step.tab) assert.ok(TAB_IDS.includes(step.tab), `step ${step.id} opens unknown tab ${step.tab}`);
      if (step.requiresTab) assert.ok(TAB_IDS.includes(step.requiresTab), `step ${step.id} requires unknown tab ${step.requiresTab}`);
    }
  });

  it('has unique step ids and a title and body for each step', () => {
    const ids = [...MAIN_TOUR, ...Object.values(PAGE_TOURS).flat()].map((s) => s.id);
    assert.equal(new Set(MAIN_TOUR.map((s) => s.id)).size, MAIN_TOUR.length, 'duplicate main-tour step ids');
    for (const s of [...MAIN_TOUR, ...Object.values(PAGE_TOURS).flat()]) {
      assert.ok(s.title && s.body && s.targets.length >= 1, `step ${s.id} is incomplete`);
    }
    assert.ok(ids.length > 0);
  });

  it('shows the admin step to admins only, and it points at the admin link', () => {
    const admin = MAIN_TOUR.filter((s) => s.role === 'admin');
    assert.equal(admin.length, 1, 'expected one admin-only step');
    assert.deepEqual(admin[0].targets, ['admin-link']);
    assert.ok(MAIN_TOUR.filter((s) => s.role !== 'admin').every((s) => s.role === undefined));
  });

  it('has page tours for the complex pages that have them', () => {
    assert.ok(PAGE_TOURS.backtests?.length >= 3);
    assert.ok(PAGE_TOURS.paper?.length >= 3);
  });
});
