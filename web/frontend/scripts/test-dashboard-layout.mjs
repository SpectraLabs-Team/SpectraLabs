// Unit tests for Dashboard Layout Engine (M18 Task 8 & 9).
// Run: node scripts/test-dashboard-layout.mjs
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

// Transpile dashboardLayout.ts to test without requiring a build step
const ts = require(path.join(root, 'node_modules/typescript'));
const tsSource = fs.readFileSync(path.join(root, 'lib/dashboardLayout.ts'), 'utf8');
const { outputText } = ts.transpileModule(tsSource, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
});

// Execute the transpiled module in a sandbox/eval or temp file
const tempDir = path.join(root, 'node_modules', '.cache', 'dashboard-layout-test');
fs.mkdirSync(tempDir, { recursive: true });
const tempFile = path.join(tempDir, 'dashboardLayout.cjs');
fs.writeFileSync(tempFile, outputText);
const {
  DEFAULT_LAYOUT,
  getLayoutStorageKey,
  encodeLayoutForSharing,
  decodeLayoutFromSharing,
  createShareableLayoutUrl,
  parseLayoutShareParams,
  resetLayout,
} = require(tempFile);

describe('Dashboard Layout Engine', () => {
  it('DEFAULT_LAYOUT contains all 6 core widgets with valid metadata', () => {
    assert.strictEqual(DEFAULT_LAYOUT.length, 6, 'Should have exactly 6 default widgets');
    const ids = DEFAULT_LAYOUT.map((w) => w.id);
    const expectedIds = [
      'summary_kpis',
      'risk_gauges',
      'recent_runs',
      'kill_switches',
      'audit_decisions',
      'quick_actions',
    ];
    assert.deepStrictEqual(ids, expectedIds, 'Widget IDs must match expected specification');

    for (const w of DEFAULT_LAYOUT) {
      assert.ok(w.title && w.title.length > 0, `Widget ${w.id} must have a title`);
      assert.ok(w.description && w.description.length > 0, `Widget ${w.id} must have a description`);
      assert.strictEqual(w.visible, true, `Widget ${w.id} should be visible by default`);
    }
  });

  it('getLayoutStorageKey handles workspace scoping', () => {
    assert.strictEqual(getLayoutStorageKey(null), 'spectra_overview_layout_default');
    assert.strictEqual(getLayoutStorageKey(undefined), 'spectra_overview_layout_default');
    assert.strictEqual(getLayoutStorageKey(''), 'spectra_overview_layout_default');
    assert.strictEqual(getLayoutStorageKey('ws-abc-123'), 'spectra_overview_layout_ws-abc-123');
  });

  it('encodeLayoutForSharing and decodeLayoutFromSharing round-trip correctly', () => {
    const custom = DEFAULT_LAYOUT.map((w, idx) => ({
      ...w,
      visible: idx % 2 === 0, // toggle alternate widgets
    }));

    const encoded = encodeLayoutForSharing(custom);
    assert.ok(typeof encoded === 'string' && encoded.length > 0);
    assert.ok(encoded.includes('summary_kpis:1'));
    assert.ok(encoded.includes('risk_gauges:0'));

    const decoded = decodeLayoutFromSharing(encoded);
    assert.ok(decoded !== null, 'Decoded layout must not be null');
    assert.strictEqual(decoded.length, 6, 'Decoded layout must retain all 6 widgets');

    for (let i = 0; i < decoded.length; i++) {
      assert.strictEqual(decoded[i].id, custom[i].id);
      assert.strictEqual(decoded[i].visible, custom[i].visible);
      assert.strictEqual(decoded[i].title, custom[i].title);
      assert.strictEqual(decoded[i].description, custom[i].description);
    }
  });

  it('decodeLayoutFromSharing recovers missing widgets from DEFAULT_LAYOUT', () => {
    // Only 2 widgets present in the URL string
    const partialEncoded = 'recent_runs:1;audit_decisions:0';
    const decoded = decodeLayoutFromSharing(partialEncoded);

    assert.ok(decoded !== null);
    assert.strictEqual(decoded.length, 6, 'Missing widgets must be auto-filled from defaults');

    // First two should be the specified ones in order
    assert.strictEqual(decoded[0].id, 'recent_runs');
    assert.strictEqual(decoded[0].visible, true);
    assert.strictEqual(decoded[1].id, 'audit_decisions');
    assert.strictEqual(decoded[1].visible, false);

    // Remaining widgets should exist and default to true
    const remainingIds = decoded.slice(2).map((w) => w.id);
    assert.ok(remainingIds.includes('summary_kpis'));
    assert.ok(remainingIds.includes('risk_gauges'));
    assert.ok(remainingIds.includes('kill_switches'));
    assert.ok(remainingIds.includes('quick_actions'));
  });

  it('decodeLayoutFromSharing handles invalid or corrupted input safely', () => {
    assert.strictEqual(decodeLayoutFromSharing(''), null);
    assert.strictEqual(decodeLayoutFromSharing('unknown_foo:1;bar:0'), null);
  });

  it('createShareableLayoutUrl generates cross-workspace links with read-only flag (M18 Task 9)', () => {
    const urlReadOnly = createShareableLayoutUrl('https://spectra.example.com/studio', DEFAULT_LAYOUT, { readOnly: true });
    assert.ok(urlReadOnly.startsWith('https://spectra.example.com/studio?tab=overview&layout='));
    assert.ok(urlReadOnly.includes('&readonly=true'));

    const parsedReadOnly = parseLayoutShareParams(new URL(urlReadOnly).search);
    assert.strictEqual(parsedReadOnly.isReadOnly, true);
    assert.ok(parsedReadOnly.layout !== null);
    assert.strictEqual(parsedReadOnly.layout.length, 6);

    const urlEditable = createShareableLayoutUrl('https://spectra.example.com/studio', DEFAULT_LAYOUT, { readOnly: false });
    assert.ok(!urlEditable.includes('&readonly=true'));
    const parsedEditable = parseLayoutShareParams(new URL(urlEditable).search);
    assert.strictEqual(parsedEditable.isReadOnly, false);
  });

  it('resetLayout returns a fresh independent copy of DEFAULT_LAYOUT', () => {
    const fresh = resetLayout();
    assert.strictEqual(fresh.length, DEFAULT_LAYOUT.length);
    assert.notStrictEqual(fresh, DEFAULT_LAYOUT, 'Must return a fresh cloned array');

    // Modifying the returned copy must not mutate DEFAULT_LAYOUT
    fresh[0].visible = false;
    assert.strictEqual(DEFAULT_LAYOUT[0].visible, true, 'DEFAULT_LAYOUT must remain immutable');
  });
});
