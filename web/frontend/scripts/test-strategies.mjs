// Strategy Studio client mappers: engine wire shapes to the Studio's shapes. No network needed.
// Run: node scripts/test-strategies.mjs
import { describe, it, after } from 'node:test';
import assert from 'node:assert/strict';
import { transpileForTest } from './transpile-for-test.mjs';

const t = transpileForTest('strategies', { files: ['lib/strategyMappers.ts'] });
after(t.cleanup);
const m = t.load('lib/strategyMappers.js');

describe('strategy version mapping', () => {
  const engine = {
    id: 'v1',
    strategy_id: 's1',
    version: 3,
    strategy_sha256: 'abc123',
    params: { fast: 10, slow: 30 },
    code: 'rule x',
    created_at: '2026-10-27T00:00:00Z',
  };

  it('maps the engine fields to the Studio names', () => {
    const v = m.toStrategyVersion(engine);
    assert.equal(v.version_num, 3);
    assert.equal(v.code_sha256, 'abc123');
    assert.deepEqual(v.parameters, { fast: 10, slow: 30 });
    assert.equal(v.code, 'rule x');
    assert.equal(v.notes, '');
  });

  it('takes the strategy id from the caller when the engine omits it', () => {
    const { strategy_id: _ignored, ...withoutId } = engine;
    assert.equal(m.toStrategyVersion(withoutId, 's9').strategy_id, 's9');
  });

  it('never leaks the engine names into the Studio shape', () => {
    const v = m.toStrategyVersion(engine);
    assert.equal('strategy_sha256' in v, false);
    assert.equal('params' in v, false);
  });

  it('uses empty parameters when the engine sends none', () => {
    assert.deepEqual(m.toStrategyVersion({ ...engine, params: undefined }).parameters, {});
  });
});

describe('diff mapping', () => {
  it('maps code lines and parameter changes', () => {
    const d = m.toStrategyDiff({
      strategy_id: 's1',
      from_version: 1,
      to_version: 2,
      code_diff: [
        { kind: 'unchanged', line_from: 1, line_to: 1, text: 'a' },
        { kind: 'removed', line_from: 2, line_to: null, text: 'b' },
        { kind: 'added', line_from: null, line_to: 2, text: 'c' },
      ],
      parameter_diff: { changed: { fast: { from: 10, to: 12 } }, added: { band: 2 }, removed: { old: 1 } },
    });
    assert.deepEqual(
      d.lines.map((l) => l.change_type),
      ['equal', 'deleted', 'added'],
    );
    assert.equal(d.lines[1].line_num_from, 2);
    assert.equal(d.lines[1].line_num_to, undefined);
    assert.deepEqual(d.parameters_changed.find((p) => p.name === 'fast'), { name: 'fast', old_value: 10, new_value: 12 });
    assert.equal(d.parameters_changed.find((p) => p.name === 'band').new_value, 2);
    assert.equal(d.parameters_changed.find((p) => p.name === 'old').old_value, 1);
  });

  it('copes with an empty diff', () => {
    const d = m.toStrategyDiff({ strategy_id: 's', from_version: 1, to_version: 1, code_diff: [], parameter_diff: undefined });
    assert.deepEqual(d.lines, []);
    assert.deepEqual(d.parameters_changed, []);
  });
});

describe('parameter set and summary mapping', () => {
  it('scopes a parameter set to the strategy it was read from', () => {
    const p = m.toParameterSet({ id: 'p', workspace_id: 'w', strategy_version_id: 'v1', name: 'Tuned', parameters: { fast: 5 }, created_at: 'x' }, 's1');
    assert.equal(p.strategy_id, 's1');
    assert.equal(p.strategy_version_id, 'v1');
    assert.deepEqual(p.parameters, { fast: 5 });
  });

  it('marks a strategy as builtin only when its latest version is a built-in', () => {
    const base = { id: 's', name: 'n', description: '', created_at: 'x' };
    assert.equal(m.toStrategySummary({ ...base, latest_builtin: 'sma_cross' }).kind, 'builtin');
    assert.equal(m.toStrategySummary({ ...base, latest_builtin: null }).kind, 'custom');
    assert.equal(m.toStrategySummary({ ...base, description: undefined }).description, '');
  });
});
