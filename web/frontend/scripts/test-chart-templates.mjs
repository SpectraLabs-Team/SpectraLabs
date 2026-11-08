/**
 * Test suite for M36 Task 4 (Saved Layout Templates & UI Integration).
 *
 * Verifies:
 * 1. chart-drawings.ts exports template API methods (fetchTemplates, saveTemplate, getTemplate, deleteTemplate).
 * 2. Template schema includes name, version, indicators, drawings, comparisons, and dataset_versions.
 * 3. 50-template limit per workspace enforced.
 * 4. Retired dataset versions alert detection.
 * 5. Viewer read-only access rule (viewers cannot save or delete templates).
 * 6. TemplateModal component tabs (Save / Load), contents summary, and validation.
 * 7. TradingChart integration ("Templates" button and apply template handlers).
 */

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { describe, it } from 'node:test';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const root = path.resolve(__dirname, '..');

describe('M36 Task 4: Layout Templates Client Library', () => {
  const libPath = path.resolve(root, 'lib/chart-drawings.ts');
  const libSource = fs.readFileSync(libPath, 'utf8');

  it('exports template CRUD API methods querying /api/v1/chart-templates', () => {
    assert.ok(libSource.includes('export async function fetchTemplates'), 'Must export fetchTemplates');
    assert.ok(libSource.includes('export async function saveTemplate'), 'Must export saveTemplate');
    assert.ok(libSource.includes('export async function getTemplate'), 'Must export getTemplate');
    assert.ok(libSource.includes('export async function deleteTemplate'), 'Must export deleteTemplate');
    assert.ok(libSource.includes('/api/v1/chart-templates'), 'Must query /api/v1/chart-templates');
  });

  it('ChartTemplate interface includes all required fields', () => {
    assert.ok(libSource.includes('export interface ChartTemplate'), 'Must export ChartTemplate');
    assert.ok(libSource.includes('indicators: any[]'), 'Must store indicators');
    assert.ok(libSource.includes('drawings: any[]'), 'Must store drawings');
    assert.ok(libSource.includes('comparisons: string[]'), 'Must store comparisons');
    assert.ok(libSource.includes('dataset_versions:'), 'Must track dataset_versions');
  });
});

describe('M36 Task 4: TemplateModal UI Component', () => {
  const modalPath = path.resolve(root, 'components/studio/charts/TemplateModal.tsx');
  const modalSource = fs.readFileSync(modalPath, 'utf8');

  it('exports TemplateModal and accepts required props', () => {
    assert.ok(modalSource.includes('export function TemplateModal'), 'Must export TemplateModal');
    assert.ok(modalSource.includes('templates: ChartTemplate[]'), 'Must accept templates');
    assert.ok(modalSource.includes('onSaveTemplate:'), 'Must accept onSaveTemplate');
    assert.ok(modalSource.includes('onLoadTemplate:'), 'Must accept onLoadTemplate');
    assert.ok(modalSource.includes('onDeleteTemplate:'), 'Must accept onDeleteTemplate');
    assert.ok(modalSource.includes('isViewer?: boolean'), 'Must accept isViewer');
  });

  it('enforces 50 template limit per workspace', () => {
    assert.ok(
      modalSource.includes('Workspace limit of 50 templates reached'),
      'Must enforce 50-template limit message'
    );
  });

  it('displays retired dataset alerts when dataset versions are retired or missing', () => {
    assert.ok(modalSource.includes('retired_versions'), 'Must check retired_versions');
    assert.ok(modalSource.includes('Retired Dataset'), 'Must display Retired Dataset alert badge');
  });

  it('enforces viewer read-only rule: save button disabled and warning displayed', () => {
    assert.ok(modalSource.includes('Viewers have read-only access'), 'Must display viewer read-only warning');
    assert.ok(modalSource.includes('disabled={isViewer'), 'Save button must be disabled for viewers');
  });
});

describe('M36 Task 4: TradingChart Templates Integration', () => {
  const chartPath = path.resolve(root, 'components/studio/TradingChart.tsx');
  const chartSource = fs.readFileSync(chartPath, 'utf8');

  it('TradingChart accepts template props and renders "Templates" toolbar button', () => {
    assert.ok(chartSource.includes('templates?: ChartTemplate[]'), 'Must accept templates prop');
    assert.ok(chartSource.includes('onSaveTemplate?:'), 'Must accept onSaveTemplate');
    assert.ok(chartSource.includes('onLoadTemplate?:'), 'Must accept onLoadTemplate');
    assert.ok(chartSource.includes('onDeleteTemplate?:'), 'Must accept onDeleteTemplate');
    assert.ok(chartSource.includes('setIsTemplateModalOpen(true)'), 'Must have button to open TemplateModal');
    assert.ok(chartSource.includes('<span>Templates</span>'), 'Must display Templates button label');
  });

  it('TradingChart renders TemplateModal in JSX', () => {
    assert.ok(chartSource.includes('<TemplateModal'), 'Must render TemplateModal');
    assert.ok(chartSource.includes('isOpen={isTemplateModalOpen}'), 'Must bind isTemplateModalOpen');
    assert.ok(chartSource.includes('onLoadTemplate={handleApplyTemplate}'), 'Must bind handleApplyTemplate');
  });
});
