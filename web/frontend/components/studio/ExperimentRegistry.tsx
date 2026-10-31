'use client';

import { MetricLabel } from '@/components/ui/MetricInfo';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useSearchParams } from 'next/navigation';
import { Activity, Beaker, CircleAlert, CircleCheck, GitFork, LoaderCircle, Plus, RefreshCw, Square } from 'lucide-react';
import { RunDetail } from './RunDetail';
import { ApiError, request } from '@/lib/api';
import { cancelJob, waitForJob, type Job } from '@/lib/jobs';

type Experiment = {
  id: string;
  name: string;
  hypothesis: string;
  tags: string[];
  parent_id: string;
  created_ts: number;
  notes: string[];
  run_count: number;
  succeeded_count: number;
};

export type Run = {
  id: string;
  experiment_id: string;
  kind: string;
  status: string;
  dataset_id: string;
  strategy: string;
  params: Record<string, number>;
  metrics: { sharpe: number; total_return: number; max_drawdown: number };
  error: string;
  recorded_ts: number;
};

type RunPage = { items: Run[]; next_cursor: string | null };

type BacktestReport = {
  run_id: string;
  report: {
    metrics?: { sharpe?: number; total_return?: number; max_drawdown?: number };
    dataset_id?: string;
    strategy?: string;
  };
};

const pct = (value: number) => `${(value * 100).toFixed(2)}%`;

export function ExperimentRegistry() {
  const searchParams = useSearchParams();
  const expParam = searchParams.get('experiment');
  const runParam = searchParams.get('run');

  const [items, setItems] = useState<Experiment[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [selected, setSelected] = useState('');
  const [lineage, setLineage] = useState<string[]>([]);
  const [name, setName] = useState('');
  const [hypothesis, setHypothesis] = useState('');
  const [tags, setTags] = useState('');
  const [parentId, setParentId] = useState('');
  const [csv, setCsv] = useState('');
  const [fast, setFast] = useState(5);
  const [slow, setSlow] = useState(20);
  const [activeJob, setActiveJob] = useState<Job<BacktestReport> | null>(null);
  const [runId, setRunId] = useState('');
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [openRun, setOpenRun] = useState(runParam ?? '');
  const [myRunsOnly, setMyRunsOnly] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');

  const selectedExperiment = useMemo(() => items.find((item) => item.id === selected) ?? null, [items, selected]);
  const childExperiments = useMemo(
    () => (selected ? items.filter((item) => item.parent_id === selected) : []),
    [items, selected]
  );

  const forkExperiment = useCallback((exp: Experiment) => {
    setParentId(exp.id);
    setName(`${exp.name} (Fork)`);
    setHypothesis(exp.hypothesis ? `Derived from ${exp.name}: ` : '');
    setTags(exp.tags.join(', '));
    setMessage(`Ready to create a child experiment branched from "${exp.name}".`);
    window.scrollTo({ top: 0, behavior: 'smooth' });
  }, []);

  const selectExperiment = useCallback((expId: string) => {
    setSelected(expId);
    setOpenRun('');
    try {
      const url = new URL(window.location.href);
      url.searchParams.set('tab', 'registry');
      url.searchParams.set('experiment', expId);
      url.searchParams.delete('run');
      window.history.replaceState(null, '', url.toString());
    } catch {
      /* ignore */
    }
  }, []);

  const selectRun = useCallback((rId: string) => {
    setOpenRun(rId);
    try {
      const url = new URL(window.location.href);
      url.searchParams.set('tab', 'registry');
      if (selected) url.searchParams.set('experiment', selected);
      url.searchParams.set('run', rId);
      window.history.replaceState(null, '', url.toString());
    } catch {
      /* ignore */
    }
  }, [selected]);

  const loadLineage = useCallback(async (experimentId: string) => {
    if (!experimentId) {
      setLineage([]);
      return;
    }
    try {
      const result = await request<{ items: string[] }>(`/api/v1/experiments/${encodeURIComponent(experimentId)}/lineage`);
      setLineage(result.items || [experimentId]);
    } catch {
      setLineage([experimentId]);
    }
  }, []);

  const loadExperiments = useCallback(async () => {
    const result = await request<{ items: Experiment[] }>('/api/v1/experiments');
    setItems(result.items);
    setSelected((current) => {
      if (expParam && result.items.some((item) => item.id === expParam)) {
        return expParam;
      }
      return current && result.items.some((item) => item.id === current) ? current : result.items[0]?.id ?? '';
    });
  }, [expParam]);

  const loadRuns = useCallback(async (experimentId: string, cursor?: string, forMyRuns = myRunsOnly) => {
    if (!forMyRuns && !experimentId) { setRuns([]); setNextCursor(null); return; }
    const query = new URLSearchParams({ limit: '25' });
    if (!forMyRuns && experimentId) query.set('experiment_id', experimentId);
    if (cursor) query.set('cursor', cursor);
    const endpoint = forMyRuns ? `/api/v1/me/runs?${query}` : `/api/v1/runs?${query}`;
    const result = await request<RunPage>(endpoint);
    setRuns((current) => (cursor ? [...current, ...result.items] : result.items));
    setNextCursor(result.next_cursor);
  }, [myRunsOnly]);

  const hideRun = useCallback(async (rId: string) => {
    try {
      await request(`/api/v1/me/runs/${encodeURIComponent(rId)}/hide`, { method: 'POST' });
      setRuns((current) => current.filter((r) => r.id !== rId));
      if (openRun === rId) setOpenRun('');
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not hide run.');
    }
  }, [openRun]);

  const refresh = useCallback(async () => {
    setError('');
    try {
      await loadExperiments();
      if (myRunsOnly || selected) {
        await loadRuns(selected, undefined, myRunsOnly);
        if (selected) await loadLineage(selected);
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not load the research registry.');
    }
  }, [loadExperiments, loadRuns, loadLineage, selected, myRunsOnly]);

  useEffect(() => { void refresh(); }, [refresh]);

  // Deep-link to a specific run if provided in the URL query (?tab=registry&run=<id>)
  useEffect(() => {
    if (!runParam) return;
    (async () => {
      try {
        const runData = await request<{ id: string; experiment_id: string }>(`/api/v1/runs/${encodeURIComponent(runParam)}`);
        if (runData.experiment_id) {
          setSelected(runData.experiment_id);
          setOpenRun(runParam);
        }
      } catch {
        /* run not found or error */
      }
    })();
  }, [runParam]);

  useEffect(() => {
    if (myRunsOnly || selected) {
      void loadRuns(selected, undefined, myRunsOnly).catch((cause) =>
        setError(cause instanceof Error ? cause.message : 'Could not load runs.')
      );
      if (selected) void loadLineage(selected);
    } else {
      setRuns([]);
      setNextCursor(null);
      setLineage([]);
    }
  }, [selected, myRunsOnly, loadRuns, loadLineage]);

  const createExperiment = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setBusy(true); setError(''); setMessage('');
    try {
      const payload: Record<string, unknown> = {
        name,
        hypothesis,
        tags: tags.split(',').map((tag) => tag.trim()).filter(Boolean),
      };
      if (parentId) payload.parent_id = parentId;
      const created = await request<{ id: string }>('/api/v1/experiments', {
        method: 'POST',
        body: JSON.stringify(payload),
      });
      setName(''); setHypothesis(''); setTags(''); setParentId('');
      await loadExperiments(); setSelected(created.id);
      setMessage(parentId ? 'Child experiment created with lineage.' : 'Root experiment created.');
    } catch (cause) {
      setError(cause instanceof ApiError ? cause.message : 'Could not create experiment.');
    } finally { setBusy(false); }
  };

  const startBacktest = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setBusy(true); setError(''); setMessage(''); setActiveJob(null); setRunId('');
    try {
      // Without a chosen experiment the run is recorded under the workspace's "Unsorted" one.
      const queued = await request<{ job: Job<BacktestReport>; run_id: string }>(
        selected ? `/api/v1/experiments/${encodeURIComponent(selected)}/runs` : '/api/v1/runs',
        { method: 'POST', body: JSON.stringify({ csv, fast, slow }), timeoutMs: 60_000 },
      );
      if (!selected) await loadExperiments();
      setActiveJob(queued.job); setRunId(queued.run_id);
      const finished = await waitForJob<BacktestReport>(queued.job.id, { timeoutMs: 30 * 60_000, onUpdate: setActiveJob });
      setActiveJob(finished);
      setMessage(finished.status === 'succeeded' ? `Backtest finished · run ${queued.run_id}` : finished.error ?? `Backtest ${finished.status}.`);
      await loadExperiments();
      if (selected || myRunsOnly) await loadRuns(selected, undefined, myRunsOnly);
      if (finished.status === 'succeeded') setOpenRun(queued.run_id);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'The backtest could not be completed.');
      if (selected || myRunsOnly) await loadRuns(selected, undefined, myRunsOnly).catch(() => undefined);
    } finally { setBusy(false); }
  };

  const stopBacktest = async () => {
    if (!activeJob) return;
    setError('');
    try { setActiveJob(await cancelJob<BacktestReport>(activeJob.id)); }
    catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not cancel the backtest.'); }
  };

  return (
    <div className="mx-auto w-full max-w-7xl space-y-6 px-4 py-6 lg:px-8">
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <p className="font-mono text-xs uppercase tracking-[0.18em] text-accent">Research workspace</p>
          <h1 className="mt-2 text-2xl font-semibold text-ink">Experiment registry</h1>
          <p className="mt-1 max-w-2xl text-sm text-ink/55">Track hypotheses and reproducible SMA crossover backtests. Runs are queued and stored with their fingerprints.</p>
        </div>
        <button type="button" onClick={() => void refresh()} className="btn-secondary inline-flex items-center gap-2" aria-label="Refresh registry">
          <RefreshCw className="h-4 w-4" /> Refresh
        </button>
      </header>

      {error && <div role="alert" className="flex gap-2 rounded-xl border border-loss/30 bg-loss/10 p-3 text-sm text-loss"><CircleAlert className="mt-0.5 h-4 w-4 shrink-0" />{error}</div>}
      {message && <div role="status" className="flex gap-2 rounded-xl border border-profit/30 bg-profit/10 p-3 text-sm text-profit"><CircleCheck className="mt-0.5 h-4 w-4 shrink-0" />{message}</div>}

      <div className="grid gap-5 xl:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)]">
        <section className="space-y-5">
          <form onSubmit={createExperiment} className="rounded-2xl border border-ink/10 bg-ink/[0.025] p-5">
            <h2 className="flex items-center gap-2 text-base font-semibold text-ink"><Plus className="h-4 w-4 text-accent" />New experiment</h2>
            <label className="mt-4 block text-xs font-medium text-ink/65">Name
              <input required maxLength={200} value={name} onChange={(event) => setName(event.target.value)} className="input mt-1 w-full" placeholder="Momentum baseline" />
            </label>
            <label className="mt-3 block text-xs font-medium text-ink/65">Hypothesis
              <textarea maxLength={10_000} value={hypothesis} onChange={(event) => setHypothesis(event.target.value)} className="input mt-1 min-h-20 w-full resize-y" placeholder="What do you expect to observe?" />
            </label>
            <label className="mt-3 block text-xs font-medium text-ink/65">Tags <span className="font-normal text-ink/40">comma separated</span>
              <input value={tags} onChange={(event) => setTags(event.target.value)} className="input mt-1 w-full" placeholder="daily, baseline" />
            </label>
            <label className="mt-3 block text-xs font-medium text-ink/65">
              Parent experiment (Lineage branch)
              <select
                value={parentId}
                onChange={(event) => setParentId(event.target.value)}
                className="input mt-1 w-full"
              >
                <option value="">None (Root experiment)</option>
                {items.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.name} {item.id === selected ? '(Selected)' : ''}
                  </option>
                ))}
              </select>
            </label>
            <button disabled={busy} className="btn-primary mt-4 inline-flex items-center gap-2"><Plus className="h-4 w-4" />Create experiment</button>
          </form>

          <div className="rounded-2xl border border-ink/10 bg-ink/[0.025] p-5">
            <h2 className="text-base font-semibold text-ink">Experiments <span className="font-mono text-xs text-ink/40">{items.length}</span></h2>
            <div className="mt-3 space-y-2">
              {items.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => selectExperiment(item.id)}
                  aria-pressed={selected === item.id}
                  className={`w-full rounded-xl border p-3 text-left transition ${selected === item.id ? 'border-accent/40 bg-accent/10' : 'border-ink/[0.08] hover:border-ink/20'}`}
                >
                  <span className="flex items-baseline justify-between gap-2">
                    <span className="font-medium text-ink">{item.name}</span>
                    <span className="shrink-0 font-mono text-[10px] text-ink/45" title={`${item.succeeded_count} succeeded`}>{item.run_count} {item.run_count === 1 ? 'run' : 'runs'}</span>
                  </span>
                  <span className="mt-1 block line-clamp-2 text-xs text-ink/50">{item.hypothesis || 'No hypothesis added.'}</span>
                  <span className="mt-2 flex items-center justify-between gap-1">
                    <span className="flex flex-wrap gap-1">{item.tags.map((tag) => <span key={tag} className="rounded-full bg-ink/5 px-2 py-0.5 font-mono text-[10px] text-ink/55">{tag}</span>)}</span>
                    <span
                      onClick={(e) => { e.stopPropagation(); forkExperiment(item); }}
                      className="rounded px-2 py-0.5 font-mono text-[10px] font-semibold text-accent/80 hover:bg-accent/10 hover:text-accent cursor-pointer"
                      title="Fork / branch a child experiment from this"
                    >
                      + Branch
                    </span>
                  </span>
                </button>
              ))}
              {!items.length && <p className="rounded-lg border border-dashed border-ink/15 p-4 text-sm text-ink/45">Create an experiment to start recording research.</p>}
            </div>
          </div>
        </section>

        <section className="space-y-5">
          {selectedExperiment && lineage.length > 0 && (
            <div className="rounded-2xl border border-ink/10 bg-ink/[0.025] p-4 text-xs">
              <div className="flex items-center justify-between gap-2">
                <span className="flex items-center gap-1.5 font-mono text-[10.5px] uppercase tracking-wider text-ink/50">
                  <GitFork className="h-3.5 w-3.5 text-accent" /> Lineage Tree
                </span>
                <div className="flex items-center gap-2">
                  <span className="font-mono text-[10px] text-ink/40">
                    {lineage.length === 1 ? 'Root node' : `${lineage.length} generations (ancestors → current)`}
                  </span>
                  <button
                    type="button"
                    onClick={() => forkExperiment(selectedExperiment)}
                    className="inline-flex items-center gap-1 rounded-md border border-accent/30 bg-accent/10 px-2 py-0.5 text-[10px] font-semibold text-accent transition hover:bg-accent/20"
                    title="Branch a new child experiment from the current one"
                  >
                    <Plus className="h-3 w-3" /> Fork / Branch
                  </button>
                </div>
              </div>

              {/* Ancestor generation pipeline */}
              <div className="mt-3 flex flex-wrap items-center gap-2">
                {lineage.map((id, idx) => {
                  const item = items.find((e) => e.id === id);
                  const label = item ? item.name : id.slice(0, 8);
                  const isCurrent = id === selected;
                  const isRoot = idx === 0;
                  const genNumber = idx + 1;
                  return (
                    <div key={id} className="flex items-center gap-2">
                      {idx > 0 && <span className="text-ink/30">→</span>}
                      {isCurrent ? (
                        <div
                          className="inline-flex items-center gap-1.5 rounded-lg border border-accent/30 bg-accent/10 px-2.5 py-1.5 font-semibold text-accent"
                          title={`Current experiment: ${id}`}
                        >
                          <span className="rounded bg-accent/20 px-1 py-0.5 font-mono text-[9px] uppercase tracking-wider text-accent">
                            Gen {genNumber}
                          </span>
                          <span>{label}</span>
                          <span className="font-mono text-[9.5px] text-accent/80">
                            ({item?.run_count ?? 0} runs)
                          </span>
                          <span className="rounded bg-accent/20 px-1 py-0.5 text-[9px] uppercase tracking-wider text-accent">
                            current
                          </span>
                        </div>
                      ) : (
                        <button
                          type="button"
                          onClick={() => selectExperiment(id)}
                          className="inline-flex items-center gap-1.5 rounded-lg border border-ink/[0.08] bg-ink/[0.03] px-2.5 py-1.5 text-ink/75 transition hover:border-ink/20 hover:bg-ink/[0.06] hover:text-ink"
                          title={`Navigate to ancestor: ${id}`}
                        >
                          <span className="rounded bg-ink/5 px-1 py-0.5 font-mono text-[9px] text-ink/45">
                            {isRoot ? 'Root' : `Gen ${genNumber}`}
                          </span>
                          <span>{label}</span>
                          <span className="font-mono text-[9.5px] text-ink/40">
                            ({item?.run_count ?? 0})
                          </span>
                        </button>
                      )}
                    </div>
                  );
                })}
              </div>

              {/* Child branch derivations if any exist */}
              {childExperiments.length > 0 && (
                <div className="mt-3 pt-3 border-t border-ink/10">
                  <div className="flex items-center gap-1.5 font-mono text-[10px] uppercase tracking-wider text-ink/45">
                    <span className="text-accent">↳</span> Child branches ({childExperiments.length})
                  </div>
                  <div className="mt-1.5 flex flex-wrap items-center gap-2">
                    {childExperiments.map((child) => (
                      <button
                        key={child.id}
                        type="button"
                        onClick={() => selectExperiment(child.id)}
                        className="inline-flex items-center gap-1.5 rounded-lg border border-ink/[0.08] bg-ink/[0.03] px-2.5 py-1 text-ink/75 transition hover:border-accent/30 hover:bg-accent/5 hover:text-accent"
                        title={`Navigate to child branch: ${child.name}`}
                      >
                        <span className="font-medium">{child.name}</span>
                        <span className="font-mono text-[9.5px] text-ink/40">
                          ({child.run_count} runs)
                        </span>
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}

          <div className="rounded-2xl border border-ink/10 bg-ink/[0.025] p-5">
            <h2 className="flex items-center gap-2 text-base font-semibold text-ink"><Beaker className="h-4 w-4 text-accent" />Run SMA crossover backtest</h2>
            <p className="mt-1 text-xs text-ink/50">{selectedExperiment ? `Recorded under ${selectedExperiment.name}.` : 'No experiment selected: the run goes to Unsorted.'} Paste OHLCV CSV, up to 10 MB.</p>
            <form onSubmit={startBacktest} className="mt-4 space-y-3">
              <label className="block text-xs font-medium text-ink/65">Dataset CSV
                <textarea required value={csv} onChange={(event) => setCsv(event.target.value)} className="input mt-1 min-h-44 w-full resize-y font-mono text-xs" placeholder={'timestamp,symbol,open,high,low,close,volume\n2026-01-01,AAPL,100,102,99,101,100000'} />
              </label>
              <div className="grid grid-cols-2 gap-3">
                <label className="text-xs font-medium text-ink/65">Fast SMA
                  <input type="number" min={1} max={999} value={fast} onChange={(event) => setFast(Number(event.target.value))} className="input mt-1 w-full" />
                </label>
                <label className="text-xs font-medium text-ink/65">Slow SMA
                  <input type="number" min={2} max={1000} value={slow} onChange={(event) => setSlow(Number(event.target.value))} className="input mt-1 w-full" />
                </label>
              </div>
              <div className="flex flex-wrap items-center gap-3">
                <button disabled={busy} className="btn-primary inline-flex items-center gap-2">
                  {busy ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Activity className="h-4 w-4" />}
                  {busy ? 'Backtest running' : 'Queue backtest'}
                </button>
                {activeJob && !['succeeded', 'failed', 'cancelled'].includes(activeJob.status) && <button type="button" onClick={() => void stopBacktest()} className="btn-secondary inline-flex items-center gap-2"><Square className="h-3.5 w-3.5" />Cancel</button>}
                {activeJob && <span className="font-mono text-xs text-ink/50">{activeJob.status} · {Math.round(activeJob.progress * 100)}%{runId ? ` · ${runId.slice(0, 8)}` : ''}</span>}
              </div>
            </form>
          </div>

          <div className="rounded-2xl border border-ink/10 bg-ink/[0.025] p-5">
            <div className="flex items-center justify-between">
              <h2 className="text-base font-semibold text-ink">
                {myRunsOnly ? 'My runs' : 'Runs'}{' '}
                {(myRunsOnly || selectedExperiment) && <span className="font-mono text-xs text-ink/40">· {runs.length}</span>}
              </h2>
              <button
                type="button"
                onClick={() => setMyRunsOnly((prev) => !prev)}
                className={`inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-xs font-medium transition ${
                  myRunsOnly
                    ? 'border-accent bg-accent/10 text-accent'
                    : 'border-ink/10 bg-ink/[0.03] text-ink/70 hover:bg-ink/[0.06]'
                }`}
                aria-pressed={myRunsOnly}
              >
                {myRunsOnly ? 'Show all runs' : 'My runs'}
              </button>
            </div>
            <div className="mt-3 overflow-x-auto">
              <table className="w-full min-w-[720px] text-left text-xs">
                <thead className="border-b border-ink/10 text-ink/45"><tr>
                  <th className="py-2 pr-3 font-medium">Status / ID</th><th className="py-2 pr-3 font-medium">Strategy</th>
                  <th className="py-2 pr-3 font-medium">Parameters</th><th className="py-2 pr-3 font-medium">Dataset</th>
                  <th className="py-2 pr-3 font-medium"><MetricLabel metric="sharpe_ratio">Sharpe</MetricLabel></th><th className="py-2 pr-3 font-medium"><MetricLabel metric="total_return">Return</MetricLabel></th>
                  <th className="py-2 pr-3 font-medium"><MetricLabel metric="max_drawdown">Drawdown</MetricLabel></th><th className="py-2 pr-3 font-medium">Date</th>
                  <th className="py-2 text-right font-medium">Action</th>
                </tr></thead>
                <tbody className="divide-y divide-ink/[0.06]">
                  {runs.map((run) => {
                    const done = run.status === 'succeeded';
                    return (
                      <tr key={run.id} onClick={() => selectRun(run.id)} aria-selected={openRun === run.id}
                        className={`cursor-pointer transition hover:bg-ink/[0.03] ${openRun === run.id ? 'bg-accent/[0.06]' : ''}`}>
                        <td className="py-3 pr-3">
                          <button type="button" onClick={(event) => { event.stopPropagation(); selectRun(run.id); }} className="text-left">
                            <span className={`block font-medium ${run.status === 'failed' ? 'text-loss' : 'text-ink'}`}>{run.status}</span>
                            <span className="font-mono text-[10px] text-ink/40">{run.id.slice(0, 12)}</span>
                          </button>
                        </td>
                        <td className="py-3 pr-3 text-ink/70">{run.strategy}</td>
                        <td className="py-3 pr-3 font-mono text-ink/70">{Object.entries(run.params ?? {}).map(([k, v]) => `${k}=${v}`).join(' ')}</td>
                        <td className="py-3 pr-3 font-mono text-ink/50" title={run.dataset_id}>{run.dataset_id ? run.dataset_id.slice(0, 10) : '—'}</td>
                        <td className="py-3 pr-3 font-mono text-ink">{done && Number.isFinite(run.metrics?.sharpe) ? run.metrics.sharpe.toFixed(3) : '—'}</td>
                        <td className="py-3 pr-3 font-mono text-ink">{done && Number.isFinite(run.metrics?.total_return) ? pct(run.metrics.total_return) : '—'}</td>
                        <td className="py-3 pr-3 font-mono text-ink">{done && Number.isFinite(run.metrics?.max_drawdown) ? pct(run.metrics.max_drawdown) : '—'}</td>
                        <td className="py-3 pr-3 text-ink/50">{new Date(run.recorded_ts / 1e6).toLocaleDateString()}</td>
                        <td className="py-3 text-right">
                          <button
                            type="button"
                            onClick={(event) => {
                              event.stopPropagation();
                              void hideRun(run.id);
                            }}
                            className="rounded border border-ink/10 px-2 py-0.5 text-[11px] text-ink/60 hover:bg-ink/[0.05] hover:text-ink"
                            title="Hide run"
                          >
                            Hide
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              {!runs.length && (
                <p className="py-6 text-center text-sm text-ink/45">
                  {myRunsOnly ? 'No runs recorded by you yet.' : 'No backtest runs recorded for this experiment yet.'}
                </p>
              )}
              {nextCursor && (
                <button type="button" onClick={() => void loadRuns(selected, nextCursor, myRunsOnly).catch((cause) => setError(cause instanceof Error ? cause.message : 'Could not load more runs.'))}
                  className="btn-secondary mt-3 w-full text-xs">Load more runs</button>
              )}
            </div>
          </div>

          {openRun && (
            <RunDetail
              runId={openRun}
              experimentName={selectedExperiment?.name ?? 'Unsorted'}
              onClose={() => selectRun('')}
              onChanged={() => { if (selected || myRunsOnly) void loadRuns(selected, undefined, myRunsOnly).catch(() => undefined); }}
            />
          )}
        </section>
      </div>
    </div>
  );
}
