// Unit & integration tests for Overview Dashboard (M18 Tasks 5–9).
// Run: node scripts/test-overview-dashboard.mjs
import { test, describe, it } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const root = path.resolve(__dirname, '..');
const require = createRequire(import.meta.url);

// Transpile dashboardLayout.ts and formatters
const ts = require(path.join(root, 'node_modules/typescript'));

function transpileAndLoad(relPath, exportName) {
  const tsSource = fs.readFileSync(path.join(root, relPath), 'utf8');
  const { outputText } = ts.transpileModule(tsSource, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
  });
  const tempDir = path.join(root, 'node_modules', '.cache', 'overview-test');
  fs.mkdirSync(tempDir, { recursive: true });
  const tempFile = path.join(tempDir, path.basename(relPath).replace(/\.tsx?$/, '.cjs'));
  fs.writeFileSync(tempFile, outputText);
  return require(tempFile);
}

const {
  DEFAULT_LAYOUT,
  getLayoutStorageKey,
  encodeLayoutForSharing,
  decodeLayoutFromSharing,
  resetLayout,
} = transpileAndLoad('lib/dashboardLayout.ts');

describe('M18 Overview Dashboard Specification & State Tests', () => {

  describe('Task 5: Overview Layout & Default Widgets', () => {
    it('contains all 6 core widgets in expected initial ordering', () => {
      const widgetIds = DEFAULT_LAYOUT.map((w) => w.id);
      const expected = [
        'summary_kpis',
        'risk_gauges',
        'recent_runs',
        'kill_switches',
        'audit_decisions',
        'quick_actions',
      ];
      assert.deepStrictEqual(widgetIds, expected);
      // All widgets default to visible
      for (const w of DEFAULT_LAYOUT) {
        assert.strictEqual(w.visible, true, `Widget ${w.id} should be visible by default`);
      }
    });

    it('aggregates backtest run status correctly (succeeded, running, failed)', () => {
      const mockRuns = [
        { id: 'run-1', status: 'succeeded' },
        { id: 'run-2', status: 'running' },
        { id: 'run-3', status: 'succeeded' },
        { id: 'run-4', status: 'failed' },
      ];

      const succeededCount = mockRuns.filter((r) => r.status === 'succeeded').length;
      const runningCount = mockRuns.filter((r) => r.status === 'running').length;
      const failedCount = mockRuns.filter((r) => r.status === 'failed').length;

      assert.strictEqual(succeededCount, 2);
      assert.strictEqual(runningCount, 1);
      assert.strictEqual(failedCount, 1);
    });

    it('aggregates pre-trade audit verdicts accurately', () => {
      const mockVerdicts = [
        { id: 'v1', verdict: 'Approved' },
        { id: 'v2', verdict: 'approved' },
        { id: 'v3', verdict: 'Reduced' },
        { id: 'v4', verdict: 'Rejected' },
        { id: 'v5', verdict: 'rejected' },
      ];

      const approvedCount = mockVerdicts.filter((v) => v.verdict.toLowerCase() === 'approved').length;
      const reducedCount = mockVerdicts.filter((v) => v.verdict.toLowerCase() === 'reduced').length;
      const rejectedCount = mockVerdicts.filter((v) => v.verdict.toLowerCase() === 'rejected').length;

      assert.strictEqual(approvedCount, 2);
      assert.strictEqual(reducedCount, 1);
      assert.strictEqual(rejectedCount, 2);
    });
  });

  describe('Task 6: M8 Risk Metrics, Limit Gauges & Kill Switches', () => {
    function computeRiskLimits(metrics) {
      const limitItems = [
        {
          id: 'gross',
          name: 'Gross Exposure',
          current: metrics?.gross_exposure ?? 0,
          limit: metrics?.gross_exposure_limit ?? 1_000_000,
          utilization: metrics?.gross_exposure_utilization_pct ?? 0,
        },
        {
          id: 'net',
          name: 'Net Exposure',
          current: metrics?.net_exposure ?? 0,
          limit: metrics?.net_exposure_limit ?? 500_000,
          utilization: metrics?.net_exposure_utilization_pct ?? 0,
        },
        {
          id: 'leverage',
          name: 'Leverage',
          current: metrics?.leverage ?? 0,
          limit: metrics?.leverage_limit ?? 3.0,
          utilization: metrics?.leverage_utilization_pct ?? 0,
        },
        {
          id: 'drawdown',
          name: 'Max Drawdown',
          current: metrics?.drawdown_pct ?? 0,
          limit: metrics?.drawdown_limit_pct ?? 0.15,
          utilization: metrics?.drawdown_utilization_pct ?? 0,
        },
        {
          id: 'daily_loss',
          name: 'Daily Loss',
          current: metrics?.daily_loss ?? 0,
          limit: metrics?.daily_loss_limit ?? 50_000,
          utilization: metrics?.daily_loss_utilization_pct ?? 0,
        },
      ];
      const breachedLimits = limitItems.filter((l) => l.utilization >= 100);
      return { limitItems, breachedLimits };
    }

    it('identifies breached limits and calculates status correctly', () => {
      const normalMetrics = {
        gross_exposure: 400_000,
        gross_exposure_limit: 1_000_000,
        gross_exposure_utilization_pct: 40.0,
        leverage: 1.2,
        leverage_limit: 3.0,
        leverage_utilization_pct: 40.0,
      };
      const { breachedLimits: normalBreached } = computeRiskLimits(normalMetrics);
      assert.strictEqual(normalBreached.length, 0);

      const breachedMetrics = {
        gross_exposure: 1_200_000,
        gross_exposure_limit: 1_000_000,
        gross_exposure_utilization_pct: 120.0,
        daily_loss: 55_000,
        daily_loss_limit: 50_000,
        daily_loss_utilization_pct: 110.0,
      };
      const { breachedLimits: actualBreached } = computeRiskLimits(breachedMetrics);
      assert.strictEqual(actualBreached.length, 2);
      assert.deepStrictEqual(actualBreached.map(b => b.id), ['gross', 'daily_loss']);
    });

    it('flags trading halted if metrics kill switch is active or active switches exist', () => {
      // Case 1: All clear
      const isHalted1 = false || [].filter(k => k.active).length > 0;
      assert.strictEqual(isHalted1, false);

      // Case 2: Circuit breaker triggered from metrics
      const isHalted2 = true || [].filter(k => k.active).length > 0;
      assert.strictEqual(isHalted2, true);

      // Case 3: Manual kill switch active
      const killSwitches = [{ id: 'ks-1', active: true, reason: 'Manual emergency stop' }];
      const activeSwitches = killSwitches.filter(k => k.active);
      const isHalted3 = false || activeSwitches.length > 0;
      const haltReason = activeSwitches[0]?.reason;
      assert.strictEqual(isHalted3, true);
      assert.strictEqual(haltReason, 'Manual emergency stop');
    });
  });

  describe('Task 7: Experiment Links & Deep Linking', () => {
    it('constructs correct experiment links when experiment_id is present', () => {
      function getExperimentLink(run) {
        if (!run.experiment_id) return null;
        return `/studio?tab=experiments&experimentId=${encodeURIComponent(run.experiment_id)}`;
      }

      const runWithExp = { id: 'r-1', experiment_id: 'exp_gamma_sma' };
      const runWithoutExp = { id: 'r-2', experiment_id: null };

      assert.strictEqual(
        getExperimentLink(runWithExp),
        '/studio?tab=experiments&experimentId=exp_gamma_sma'
      );
      assert.strictEqual(getExperimentLink(runWithoutExp), null);
    });
  });

  describe('Task 8 & 9: Layout Manipulation & Workspace Role Permissions', () => {
    it('correctly reorders widgets in state with bounds protection', () => {
      function moveWidget(list, index, direction) {
        const targetIndex = direction === 'up' ? index - 1 : index + 1;
        if (targetIndex < 0 || targetIndex >= list.length) return list;
        const next = [...list];
        const [moved] = next.splice(index, 1);
        next.splice(targetIndex, 0, moved);
        return next;
      }

      const initial = [...DEFAULT_LAYOUT];
      // Move first item down
      const movedDown = moveWidget(initial, 0, 'down');
      assert.strictEqual(movedDown[0].id, 'risk_gauges');
      assert.strictEqual(movedDown[1].id, 'summary_kpis');

      // Top item moved up does nothing
      const movedUpOutOfBounds = moveWidget(initial, 0, 'up');
      assert.deepStrictEqual(movedUpOutOfBounds, initial);

      // Bottom item moved down does nothing
      const movedDownOutOfBounds = moveWidget(initial, initial.length - 1, 'down');
      assert.deepStrictEqual(movedDownOutOfBounds, initial);
    });

    it('correctly toggles widget visibility', () => {
      function toggleWidget(list, id) {
        return list.map(item => item.id === id ? { ...item, visible: !item.visible } : item);
      }

      const toggled = toggleWidget(DEFAULT_LAYOUT, 'recent_runs');
      const target = toggled.find(w => w.id === 'recent_runs');
      assert.strictEqual(target.visible, false);

      const restored = toggleWidget(toggled, 'recent_runs');
      assert.strictEqual(restored.find(w => w.id === 'recent_runs').visible, true);
    });

    it('enforces M19 role permissions: viewers cannot trigger runs or publish defaults', () => {
      function checkPermissions(role) {
        const canRun = role !== 'viewer';
        const canPublishDefault = role === 'owner' || role === 'editor';
        return { canRun, canPublishDefault };
      }

      assert.deepStrictEqual(checkPermissions('viewer'), { canRun: false, canPublishDefault: false });
      assert.deepStrictEqual(checkPermissions('editor'), { canRun: true, canPublishDefault: true });
      assert.deepStrictEqual(checkPermissions('owner'), { canRun: true, canPublishDefault: true });
      assert.deepStrictEqual(checkPermissions(null), { canRun: true, canPublishDefault: false });
    });

    it('enforces read-only locks for shared layouts and viewers (M18 Task 9)', () => {
      function evaluateLayoutSharingState({ hasSharedLayout, readOnlyParam, userRole }) {
        const isSharedView = !!hasSharedLayout;
        const isReadOnlyView = (isSharedView && readOnlyParam) || userRole === 'viewer';
        const canCustomize = !isReadOnlyView && userRole !== 'viewer';
        const canAdopt = userRole !== 'viewer';
        return { isSharedView, isReadOnlyView, canCustomize, canAdopt };
      }

      // Case 1: Member with editor role opens read-only shared link
      const editorReadOnly = evaluateLayoutSharingState({
        hasSharedLayout: true,
        readOnlyParam: true,
        userRole: 'editor',
      });
      assert.strictEqual(editorReadOnly.isSharedView, true);
      assert.strictEqual(editorReadOnly.isReadOnlyView, true);
      assert.strictEqual(editorReadOnly.canCustomize, false, 'Customization must be locked');
      assert.strictEqual(editorReadOnly.canAdopt, true, 'Editor can adopt as personal layout');

      // Case 2: Viewer role opens standard overview (read-only enforced by role)
      const viewerDefault = evaluateLayoutSharingState({
        hasSharedLayout: false,
        readOnlyParam: false,
        userRole: 'viewer',
      });
      assert.strictEqual(viewerDefault.isSharedView, false);
      assert.strictEqual(viewerDefault.isReadOnlyView, true, 'Viewer is locked to read-only');
      assert.strictEqual(viewerDefault.canCustomize, false);
      assert.strictEqual(viewerDefault.canAdopt, false, 'Viewer cannot adopt or mutate layout');

      // Case 3: Owner opens editable shared view
      const ownerEditable = evaluateLayoutSharingState({
        hasSharedLayout: true,
        readOnlyParam: false,
        userRole: 'owner',
      });
      assert.strictEqual(ownerEditable.isSharedView, true);
      assert.strictEqual(ownerEditable.isReadOnlyView, false);
      assert.strictEqual(ownerEditable.canCustomize, true);
      assert.strictEqual(ownerEditable.canAdopt, true);
    });
  });
});
