'use client';

import type { MetricId } from '@/lib/learn/metrics';
import { InfoTip } from '@/components/ui/MetricInfo';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { ArrowRight, Download, LoaderCircle, ShieldAlert } from 'lucide-react';
import { Bar, BarChart, CartesianGrid, Line, LineChart, ReferenceLine, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { API_BASE, request } from '@/lib/api';
import { cancelJob, waitForJob, type Job } from '@/lib/jobs';

type SurvivalReport = {
  id: string;
  run_id: string;
  status: string;
  worst_verdict: string | null;
  verdicts: Record<string, string>;
  results: Record<string, unknown> | null;
  config: Record<string, unknown>;
  error: string | null;
  created_at: string;
};
type SurvivalPreset = { id: string | null; name: string; config: Record<string, unknown>; source?: string };
type Queued = { report_id: string; job: Job<{ report_id: string; run_id: string; worst_verdict: string }> };

const TESTS: [string, string][] = [
  ['costs', 'Cost sensitivity'], ['delay', 'Execution delay'], ['stability', 'Parameter stability'],
  ['time', 'Time periods'], ['trades', 'Trade bootstrap'], ['sample', 'Sample adequacy'],
  ['regimes', 'Market regimes'], ['benchmark', 'Benchmark relative'], ['risk', 'Drawdown and tail risk'],
];
const color: Record<string, string> = {
  pass: 'border-profit/25 bg-profit/10 text-profit', warn: 'border-warning/25 bg-warning/10 text-warning',
  fail: 'border-loss/25 bg-loss/10 text-loss', insufficient: 'border-ink/15 bg-ink/[0.04] text-ink/55',
};
const asObject = (value: unknown): Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
const asRows = (value: unknown): Record<string, unknown>[] => Array.isArray(value) ? value.map(asObject) : [];
const asSeries = (value: unknown): number[] => Array.isArray(value) ? value.filter((item): item is number => typeof item === 'number' && Number.isFinite(item)) : [];

function ChartCard({ title, info, children }: { title: string; info?: MetricId; children: React.ReactNode }) {
  return <div className="rounded-xl border border-ink/[0.08] bg-surface/60 p-3">
    <h4 className="mb-2 flex items-center gap-1 text-xs font-semibold text-ink/70">{title}{info && <InfoTip metric={info} />}</h4>
    <div className="h-40 w-full">{children}</div>
  </div>;
}

function SurvivalCharts({ results }: { results: Record<string, unknown> }) {
  const costs = asRows(asObject(results.costs).levels).map((item) => ({ x: Number(item.extra_bps ?? 0), sharpe: Number(item.sharpe ?? 0), return: Number(item.total_return ?? 0) }));
  const executionCosts = asRows(asObject(results.costs).execution_levels).map((item, index) => ({
    x: `Scenario ${index + 1}`,
    scenario: `Scenario ${index + 1}: Borrow ${Number(item.borrow_rate ?? 0) * 100}% · impact ${Number(item.impact_coefficient ?? 0)} · participation ${Number(item.max_participation ?? 0) * 100}%`,
    sharpe: Number(item.sharpe ?? 0),
  }));
  const delays = asRows(asObject(results.delay).levels).map((item) => ({ x: Number(item.delay_bars ?? 0), sharpe: Number(item.sharpe ?? 0), return: Number(item.total_return ?? 0) }));
  const neighbours = asRows(asObject(results.stability).neighbours).map((item, index) => ({
    x: `Neighbor ${index + 1}`,
    label: `Neighbor ${index + 1} (parameter perturbation): ${String(item.params ?? '')}`,
    sharpe: Number(item.sharpe ?? 0),
  }));
  const periods = asRows(asObject(results.time).subperiods).map((item, index) => ({
    x: `Period ${index + 1}`,
    label: `Sub-period ${index + 1} out-of-sample window`,
    sharpe: Number(item.sharpe ?? 0),
    return: Number(item.total_return ?? 0),
  }));
  const histogram = asRows(asObject(asObject(results.trades).resampling).bootstrap_terminal_histogram).map((item) => ({ x: Number(item.lower ?? 0), count: Number(item.count ?? 0) }));
  const tradePnls = asSeries(results.trade_pnl_sorted).map((pnl, index) => ({ x: index + 1, pnl }));
  const regimes = asRows(asObject(results.regimes).buckets).map((item) => ({ x: String(item.name ?? ''), sharpe: Number(item.sharpe ?? 0), mean: Number(item.mean_return ?? 0) }));
  const marketStates = asRows(asObject(results.regimes).market_states).map((item) => ({ x: String(item.name ?? ''), sharpe: Number(item.sharpe ?? 0), mean: Number(item.mean_return ?? 0) }));
  const baseline = asSeries(results.baseline_equity);
  const benchmark = asSeries(results.benchmark_equity);
  const benchmarkCurve = Array.from({ length: Math.min(baseline.length, benchmark.length) }, (_, index) => ({ x: index, strategy: baseline[index], benchmark: benchmark[index] }));
  const drawdowns = asSeries(results.drawdown_curve).map((drawdown, index) => ({ x: index, drawdown }));
  const confidence = asObject(asObject(results.statistical_strength).return_block_bootstrap);
  const ci = [
    { x: 'Terminal return', low: Number(asObject(confidence.terminal_return).p05 ?? 0), median: Number(asObject(confidence.terminal_return).p50 ?? 0), high: Number(asObject(confidence.terminal_return).p95 ?? 0) },
    { x: 'Annualized Sharpe', low: Number(asObject(confidence.annualized_sharpe).p05 ?? 0), median: Number(asObject(confidence.annualized_sharpe).p50 ?? 0), high: Number(asObject(confidence.annualized_sharpe).p95 ?? 0) },
    { x: 'Maximum drawdown', low: Number(asObject(confidence.max_drawdown).p05 ?? 0), median: Number(asObject(confidence.max_drawdown).p50 ?? 0), high: Number(asObject(confidence.max_drawdown).p95 ?? 0) },
  ];
  const label = { fill: 'var(--ink-muted, #71717a)', fontSize: 9 };
  const grid = <CartesianGrid strokeDasharray="3 3" stroke="rgba(120,120,140,.16)" />;
  const zero = <ReferenceLine y={0} stroke="#94a3b8" strokeDasharray="3 3" />;
  return <div className="mt-4 grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
    <ChartCard title="Costs · Sharpe and total return vs extra slippage" info="sharpe_ratio">
      <ResponsiveContainer width="100%" height="100%"><LineChart data={costs}>{grid}<XAxis dataKey="x" tick={label} /><YAxis yAxisId="left" tick={label} width={32} /><YAxis yAxisId="right" orientation="right" tick={label} width={32} /><Tooltip /><ReferenceLine x={5} stroke="#f59e0b" strokeDasharray="4 3" /><Line yAxisId="left" dataKey="sharpe" stroke="#8b5cf6" dot={false} /><Line yAxisId="right" dataKey="return" stroke="#14b8a6" dot={false} /></LineChart></ResponsiveContainer>
    </ChartCard>
    <ChartCard title="Execution costs · borrow, volume impact and partial fills">
      <ResponsiveContainer width="100%" height="100%"><BarChart data={executionCosts}>{grid}<XAxis dataKey="x" tick={label} /><YAxis tick={label} width={32} />{zero}<Tooltip labelFormatter={(value) => executionCosts.find((item) => item.x === value)?.scenario ?? String(value)} /><Bar dataKey="sharpe" fill="#f97316" /></BarChart></ResponsiveContainer>
    </ChartCard>
    <ChartCard title="Execution delay · Sharpe by bars" info="sharpe_ratio">
      <ResponsiveContainer width="100%" height="100%"><LineChart data={delays}>{grid}<XAxis dataKey="x" tick={label} /><YAxis tick={label} width={32} />{zero}<Tooltip /><Line dataKey="sharpe" stroke="#8b5cf6" dot /></LineChart></ResponsiveContainer>
    </ChartCard>
    <ChartCard title="Parameter stability · neighbor Sharpe" info="sharpe_ratio">
      <ResponsiveContainer width="100%" height="100%"><BarChart data={neighbours}>{grid}<XAxis dataKey="x" tick={label} /><YAxis tick={label} width={32} />{zero}<Tooltip labelFormatter={(value) => neighbours.find((item) => item.x === value)?.label ?? String(value)} /><Bar dataKey="sharpe" fill="#8b5cf6" /></BarChart></ResponsiveContainer>
    </ChartCard>
    <ChartCard title="Time-period robustness · sub-period Sharpe" info="sharpe_ratio">
      <ResponsiveContainer width="100%" height="100%"><BarChart data={periods}>{grid}<XAxis dataKey="x" tick={label} /><YAxis tick={label} width={32} />{zero}<Tooltip labelFormatter={(value) => periods.find((item) => item.x === value)?.label ?? String(value)} /><Bar dataKey="sharpe" fill="#14b8a6" /></BarChart></ResponsiveContainer>
    </ChartCard>
    <ChartCard title="Trade bootstrap · terminal P&L histogram">
      <ResponsiveContainer width="100%" height="100%"><BarChart data={histogram}>{grid}<XAxis dataKey="x" type="number" tick={label} /><YAxis tick={label} width={32} /><ReferenceLine x={0} stroke="#ef4444" strokeDasharray="4 3" /><Tooltip /><Bar dataKey="count" fill="#6366f1" /></BarChart></ResponsiveContainer>
    </ChartCard>
    <ChartCard title="Concentration · sorted closed-trade P&L">
      <ResponsiveContainer width="100%" height="100%"><LineChart data={tradePnls}>{grid}<XAxis dataKey="x" tick={label} /><YAxis tick={label} width={42} />{zero}<Tooltip /><Line dataKey="pnl" stroke="#f59e0b" dot={false} /></LineChart></ResponsiveContainer>
    </ChartCard>
    <ChartCard title="Regime sensitivity · Sharpe per bucket" info="sharpe_ratio">
      <ResponsiveContainer width="100%" height="100%"><BarChart data={regimes}>{grid}<XAxis dataKey="x" tick={label} /><YAxis tick={label} width={32} />{zero}<Tooltip /><Bar dataKey="sharpe" fill="#0ea5e9" /></BarChart></ResponsiveContainer>
    </ChartCard>
    <ChartCard title="Trend, range and crisis · Sharpe by market state" info="sharpe_ratio">
      <ResponsiveContainer width="100%" height="100%"><BarChart data={marketStates}>{grid}<XAxis dataKey="x" tick={label} /><YAxis tick={label} width={32} />{zero}<Tooltip /><Bar dataKey="sharpe" fill="#0ea5e9" /></BarChart></ResponsiveContainer>
    </ChartCard>
    <ChartCard title="Benchmark · rebased equity">
      <ResponsiveContainer width="100%" height="100%"><LineChart data={benchmarkCurve}>{grid}<XAxis dataKey="x" tick={label} /><YAxis tick={label} width={42} /><Tooltip /><Line dataKey="strategy" stroke="#8b5cf6" dot={false} /><Line dataKey="benchmark" stroke="#14b8a6" dot={false} /></LineChart></ResponsiveContainer>
    </ChartCard>
    <ChartCard title="Risk · drawdown path" info="max_drawdown">
      <ResponsiveContainer width="100%" height="100%"><LineChart data={drawdowns}>{grid}<XAxis dataKey="x" tick={label} /><YAxis tick={label} width={42} /><ReferenceLine y={0.2} stroke="#f59e0b" strokeDasharray="4 3" /><ReferenceLine y={0.4} stroke="#ef4444" strokeDasharray="4 3" /><Tooltip /><Line dataKey="drawdown" stroke="#ef4444" dot={false} /></LineChart></ResponsiveContainer>
    </ChartCard>
    <ChartCard title="Return block bootstrap · empirical 5th/50th/95th percentiles">
      <ResponsiveContainer width="100%" height="100%"><BarChart data={ci}>{grid}<XAxis dataKey="x" tick={label} /><YAxis tick={label} width={42} /><ReferenceLine y={0} stroke="#94a3b8" strokeDasharray="3 3" /><Tooltip /><Bar dataKey="low" fill="#f97316" /><Bar dataKey="median" fill="#0ea5e9" /><Bar dataKey="high" fill="#22c55e" /></BarChart></ResponsiveContainer>
    </ChartCard>
  </div>;
}

export function SurvivalLabPanel({ runId, enabled, strategy, params, datasetId }: {
  runId: string; enabled: boolean; strategy: string; params: Record<string, number | boolean>; datasetId: string;
}) {
  const [reports, setReports] = useState<SurvivalReport[]>([]);
  const [presets, setPresets] = useState<SurvivalPreset[]>([]);
  const [presetId, setPresetId] = useState('builtin:Default');
  const [presetName, setPresetName] = useState('');
  const [advanced, setAdvanced] = useState(false);
  const [configText, setConfigText] = useState('');
  const [selected, setSelected] = useState('');
  const [comparison, setComparison] = useState('');
  const [progress, setProgress] = useState<Job | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  const load = useCallback(async () => {
    const list = await request<SurvivalReport[]>(`/api/v1/runs/${encodeURIComponent(runId)}/survival`);
    setReports(list);
    setSelected((current) => current && list.some((item) => item.id === current) ? current : list[0]?.id ?? '');
  }, [runId]);
  useEffect(() => { void load().catch((e) => setError(e instanceof Error ? e.message : 'Could not load survival reports.')); }, [load]);
  const loadPresets = useCallback(async () => {
    const list = await request<SurvivalPreset[]>('/api/v1/survival/presets');
    setPresets(list);
    setPresetId((current) => current && list.some((item) => (item.id ?? `builtin:${item.name}`) === current) ? current : 'builtin:Default');
  }, []);
  useEffect(() => { void loadPresets().catch((e) => setError(e instanceof Error ? e.message : 'Could not load survival presets.')); }, [loadPresets]);

  const active = reports.find((item) => item.id === selected) ?? null;
  const chosenPreset = presets.find((item) => (item.id ?? `builtin:${item.name}`) === presetId) ?? null;
  const previous = reports.find((item) => item.id === comparison) ?? null;
  useEffect(() => {
    if (chosenPreset) setConfigText(JSON.stringify(chosenPreset.config, null, 2));
  }, [chosenPreset]);
  const compareVerdict = useMemo(() => {
    if (!active || !previous) return null;
    return TESTS.map(([key, label]) => ({ label, current: active.verdicts[key] ?? '—', previous: previous.verdicts[key] ?? '—' }));
  }, [active, previous]);

  const runLab = async () => {
    setBusy(true); setError('');
    try {
      const preset = presets.find((item) => (item.id ?? `builtin:${item.name}`) === presetId);
      const customConfig = advanced ? JSON.parse(configText) as Record<string, unknown> : null;
      const body = customConfig ? { config: customConfig } : preset?.id ? { preset_id: preset.id } : preset ? { config: preset.config } : {};
      const queued = await request<Queued>(`/api/v1/runs/${encodeURIComponent(runId)}/survival`, {
        method: 'POST', body: JSON.stringify(body), timeoutMs: 20_000,
      });
      setProgress(queued.job);
      await waitForJob(queued.job.id, { timeoutMs: 15 * 60_000, onUpdate: setProgress });
      await load(); setSelected(queued.report_id);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Survival analysis did not finish.');
      await load().catch(() => undefined);
    } finally { setBusy(false); setProgress(null); }
  };
  const cancel = async () => {
    if (!progress) return;
    try { await cancelJob(progress.id); }
    catch (e) { setError(e instanceof Error ? e.message : 'Could not cancel survival analysis.'); }
  };

  const savePreset = async () => {
    if (!presetName.trim()) return;
    setError('');
    try {
      const config = advanced ? JSON.parse(configText) as Record<string, unknown> : active?.config ?? chosenPreset?.config;
      if (!config) throw new Error('Choose a configuration to save.');
      await request('/api/v1/survival/presets', { method: 'POST', body: JSON.stringify({ name: presetName.trim(), config }) });
      setPresetName(''); await loadPresets();
    } catch (e) { setError(e instanceof Error ? e.message : 'Could not save preset.'); }
  };

  return (
    <section className="mt-6 rounded-2xl border border-accent/20 bg-accent/[0.025] p-4 sm:p-5" aria-label="Strategy Survival Lab">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h3 className="flex items-center gap-2 text-sm font-semibold text-ink"><ShieldAlert className="h-4 w-4 text-accent" />Strategy Survival Lab</h3>
          <p className="mt-1 text-xs text-ink/50">Stress-test this run across nine independent checks. No combined score.</p>
        </div>
        <button type="button" disabled={!enabled || busy} onClick={() => void runLab()} className="btn-primary inline-flex items-center gap-2 text-xs">
          {busy ? <LoaderCircle className="h-3.5 w-3.5 animate-spin" /> : <ArrowRight className="h-3.5 w-3.5" />}
          {busy ? `Running · ${Math.round((progress?.progress ?? 0) * 100)}%` : 'Stress-test this run'}
        </button>
      </div>
      <p className="mt-2 text-[11px] text-ink/55">{strategy} · {Object.entries(params).map(([key, value]) => `${key}=${value}`).join(' ') || 'default parameters'} · dataset {datasetId || '—'}</p>
      {!enabled && <p className="mt-3 text-xs text-ink/45">Run the backtest successfully before starting survival analysis.</p>}
      {error && <p role="alert" className="mt-3 rounded-lg border border-loss/25 bg-loss/10 p-2 text-xs text-loss">{error}</p>}
      {busy && <div className="mt-3 rounded-xl bg-ink/[0.03] p-3">
        <div className="flex items-center justify-between gap-2"><p className="text-xs text-ink/65">Nine-test progress · {progress?.status ?? 'queued'} · {Math.round((progress?.progress ?? 0) * 100)}%</p>
          <button type="button" onClick={() => void cancel()} className="btn-secondary inline-flex items-center gap-1 text-[10px]">Cancel</button></div>
        <div className="mt-2 grid grid-cols-3 gap-1.5 sm:grid-cols-9">{TESTS.map(([key, label], index) => {
          const step = (progress?.progress ?? 0) * TESTS.length;
          const state = progress?.status === 'succeeded' || index < step ? 'done' : index === Math.floor(step) && progress?.status === 'running' ? 'running' : 'queued';
          return <div key={key} className={`rounded-md px-2 py-1 text-center text-[9px] ${state === 'done' ? 'bg-profit/10 text-profit' : state === 'running' ? 'bg-accent/10 text-accent' : 'bg-ink/[0.04] text-ink/40'}`}>{label} · {state}</div>;
        })}</div>
      </div>}
      <div className="mt-3 flex flex-wrap items-end gap-2">
        <label className="min-w-48 flex-1 text-[10px] uppercase tracking-wider text-ink/45">Configuration preset
          <select value={presetId} onChange={(e) => setPresetId(e.target.value)} className="input mt-1 block w-full text-xs">
            {presets.map((item) => <option key={item.id ?? `builtin:${item.name}`} value={item.id ?? `builtin:${item.name}`}>{item.name}{item.source === 'workspace' ? ' · workspace' : ''}</option>)}
          </select>
        </label>
        <label className="min-w-40 flex-1 text-[10px] uppercase tracking-wider text-ink/45">Save current report config
          <input value={presetName} onChange={(e) => setPresetName(e.target.value)} maxLength={80} placeholder="Preset name" className="input mt-1 block w-full text-xs" />
        </label>
        <button type="button" disabled={!configText || !presetName.trim()} onClick={() => void savePreset()} className="btn-secondary text-xs">Save preset</button>
      </div>
      <details className="mt-2" open={advanced} onToggle={(event) => setAdvanced(event.currentTarget.open)}>
        <summary className="cursor-pointer text-xs font-medium text-ink/55">Advanced thresholds and grids</summary>
        <label className="mt-2 block text-[10px] uppercase tracking-wider text-ink/45">SurvivalConfig JSON
          <textarea value={configText} onChange={(event) => setConfigText(event.target.value)} rows={10} spellCheck={false} className="input mt-1 block w-full resize-y font-mono text-[10px] normal-case tracking-normal" aria-label="Advanced survival thresholds and grids" />
        </label>
      </details>
      {reports.length > 0 && (
        <div className="mt-4 grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto]">
          <label className="text-[10px] uppercase tracking-wider text-ink/45">Report history
            <select value={selected} onChange={(e) => setSelected(e.target.value)} className="input mt-1 block w-full text-xs">
              {reports.map((item) => <option key={item.id} value={item.id}>{new Date(item.created_at).toLocaleString()} · {item.status} · {item.worst_verdict ?? 'pending'}</option>)}
            </select>
          </label>
          {active?.status === 'succeeded' && <div className="flex items-end gap-2">
            <a className="btn-secondary inline-flex items-center gap-1.5 text-xs" href={`${API_BASE}/api/v1/survival/${active.id}`} download={`survival-${active.id}.json`}><Download className="h-3.5 w-3.5" />JSON</a>
            <a className="btn-secondary inline-flex items-center gap-1.5 text-xs" href={`${API_BASE}/api/v1/survival/${active.id}/text`} download={`survival-${active.id}.txt`}><Download className="h-3.5 w-3.5" />Text</a>
          </div>}
        </div>
      )}
      {active && active.status === 'succeeded' && <>
        <p className="mt-3 rounded-lg bg-ink/[0.03] p-2 text-xs text-ink/60">Worst verdict: <strong className="capitalize">{active.worst_verdict}</strong>. There is no overall score: a strong test must not hide a fatal weakness in another.</p>
        <div className="mt-4 grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-9">
          {TESTS.map(([key, label]) => {
            const verdict = active.verdicts[key] ?? 'insufficient';
            return <div key={key} className={`rounded-xl border p-2.5 ${color[verdict] ?? color.insufficient}`} title={label}>
              <p className="truncate text-[10px] opacity-75">{label}</p><p className="mt-1 text-xs font-semibold capitalize">{verdict}</p>
            </div>;
          })}
        </div>
        <div className="mt-4 flex flex-wrap items-center gap-3">
          <p className="text-xs text-ink/70">Most severe finding: <strong className="capitalize">{active.worst_verdict}</strong></p>
          {reports.length > 1 && <label className="ml-auto text-[10px] text-ink/45">Compare with
            <select value={comparison} onChange={(e) => setComparison(e.target.value)} className="input ml-2 text-xs">
              <option value="">Choose a previous report</option>
              {reports.filter((item) => item.id !== active.id && item.status === 'succeeded').map((item) => <option key={item.id} value={item.id}>{new Date(item.created_at).toLocaleString()}</option>)}
            </select>
          </label>}
        </div>
        {compareVerdict && <div className="mt-3 grid grid-cols-3 gap-2 rounded-xl bg-ink/[0.025] p-3 text-xs">
          <span className="text-ink/45">Check</span><span className="text-ink/45">Current</span><span className="text-ink/45">Compared</span>
          {compareVerdict.map((item) => <div key={item.label} className="contents"><span>{item.label}</span><span className="capitalize">{item.current}</span><span className="capitalize">{item.previous}</span></div>)}
        </div>}
        {active.results && <SurvivalCharts results={active.results} />}
        {Array.isArray(active.results?.assumptions) && <details className="mt-3">
          <summary className="cursor-pointer text-xs font-medium text-ink/60">Assumptions and limitations</summary>
          <ul className="mt-2 list-disc space-y-1 pl-5 text-xs text-ink/55">{(active.results.assumptions as string[]).map((assumption, index) => <li key={index}>{assumption}</li>)}</ul>
        </details>}
        <details className="mt-3">
          <summary className="cursor-pointer text-xs font-medium text-ink/60">Detailed results</summary>
          <pre className="mt-2 max-h-72 overflow-auto rounded-xl bg-ink/[0.04] p-3 text-[10px] text-ink/70">{JSON.stringify(active.results, null, 2)}</pre>
        </details>
      </>}
      {active && ['queued', 'running'].includes(active.status) && <p className="mt-3 text-xs text-ink/55">Survival analysis is {active.status}. Refresh this run to see its progress.</p>}
      {active?.status === 'failed' && <p className="mt-3 text-xs text-loss">{active.error ?? 'Survival analysis failed.'}</p>}
    </section>
  );
}
