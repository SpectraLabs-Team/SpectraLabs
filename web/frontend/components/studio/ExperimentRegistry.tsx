'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import { Activity, Beaker, CircleAlert, CircleCheck, LoaderCircle, Plus, RefreshCw, Square } from 'lucide-react';
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
  const [items, setItems] = useState<Experiment[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [selected, setSelected] = useState('');
  const [name, setName] = useState('');
  const [hypothesis, setHypothesis] = useState('');
  const [tags, setTags] = useState('');
  const [csv, setCsv] = useState('');
  const [fast, setFast] = useState(5);
  const [slow, setSlow] = useState(20);
  const [activeJob, setActiveJob] = useState<Job<BacktestReport> | null>(null);
  const [runId, setRunId] = useState('');
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [openRun, setOpenRun] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');

  const selectedExperiment = useMemo(() => items.find((item) => item.id === selected) ?? null, [items, selected]);

  const loadExperiments = useCallback(async () => {
    const result = await request<{ items: Experiment[] }>('/api/v1/experiments');
    setItems(result.items);
    setSelected((current) => current && result.items.some((item) => item.id === current) ? current : result.items[0]?.id ?? '');
  }, []);

  const loadRuns = useCallback(async (experimentId: string, cursor?: string) => {
    if (!experimentId) { setRuns([]); setNextCursor(null); return; }
    const query = new URLSearchParams({ experiment_id: experimentId, limit: '25' });
    if (cursor) query.set('cursor', cursor);
    const result = await request<RunPage>(`/api/v1/runs?${query}`);
    setRuns((current) => (cursor ? [...current, ...result.items] : result.items));
    setNextCursor(result.next_cursor);
  }, []);

  const refresh = useCallback(async () => {
    setError('');
    try {
      await loadExperiments();
      if (selected) await loadRuns(selected);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not load the research registry.');
    }
  }, [loadExperiments, loadRuns, selected]);

  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => {
    setOpenRun('');
    if (selected) void loadRuns(selected).catch((cause) => setError(cause instanceof Error ? cause.message : 'Could not load runs.'));
  }, [selected, loadRuns]);

  const createExperiment = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setBusy(true); setError(''); setMessage('');
    try {
      const created = await request<{ id: string }>('/api/v1/experiments', {
        method: 'POST', body: JSON.stringify({ name, hypothesis, tags: tags.split(',').map((tag) => tag.trim()).filter(Boolean) }),
      });
      setName(''); setHypothesis(''); setTags('');
      await loadExperiments(); setSelected(created.id);
      setMessage('Experiment created.');
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
      if (selected) await loadRuns(selected);
      if (finished.status === 'succeeded') setOpenRun(queued.run_id);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'The backtest could not be completed.');
      if (selected) await loadRuns(selected).catch(() => undefined);
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
            <button disabled={busy} className="btn-primary mt-4 inline-flex items-center gap-2"><Plus className="h-4 w-4" />Create experiment</button>
          </form>

          <div className="rounded-2xl border border-ink/10 bg-ink/[0.025] p-5">
            <h2 className="text-base font-semibold text-ink">Experiments <span className="font-mono text-xs text-ink/40">{items.length}</span></h2>
            <div className="mt-3 space-y-2">
              {items.map((item) => <button key={item.id} type="button" onClick={() => setSelected(item.id)} aria-pressed={selected === item.id}
                className={`w-full rounded-xl border p-3 text-left transition ${selected === item.id ? 'border-accent/40 bg-accent/10' : 'border-ink/[0.08] hover:border-ink/20'}`}>
                <span className="flex items-baseline justify-between gap-2">
                  <span className="font-medium text-ink">{item.name}</span>
                  <span className="shrink-0 font-mono text-[10px] text-ink/45" title={`${item.succeeded_count} succeeded`}>{item.run_count} {item.run_count === 1 ? 'run' : 'runs'}</span>
                </span>
                <span className="mt-1 block line-clamp-2 text-xs text-ink/50">{item.hypothesis || 'No hypothesis added.'}</span>
                <span className="mt-2 flex flex-wrap gap-1">{item.tags.map((tag) => <span key={tag} className="rounded-full bg-ink/5 px-2 py-0.5 font-mono text-[10px] text-ink/55">{tag}</span>)}</span>
              </button>)}
              {!items.length && <p className="rounded-lg border border-dashed border-ink/15 p-4 text-sm text-ink/45">Create an experiment to start recording research.</p>}
            </div>
          </div>
        </section>

        <section className="space-y-5">
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
            <h2 className="text-base font-semibold text-ink">Runs {selectedExperiment && <span className="font-mono text-xs text-ink/40">· {runs.length}</span>}</h2>
            <div className="mt-3 overflow-x-auto">
              <table className="w-full min-w-[720px] text-left text-xs">
                <thead className="border-b border-ink/10 text-ink/45"><tr>
                  <th className="py-2 pr-3 font-medium">Status / ID</th><th className="py-2 pr-3 font-medium">Strategy</th>
                  <th className="py-2 pr-3 font-medium">Parameters</th><th className="py-2 pr-3 font-medium">Dataset</th>
                  <th className="py-2 pr-3 font-medium">Sharpe</th><th className="py-2 pr-3 font-medium">Return</th>
                  <th className="py-2 pr-3 font-medium">Drawdown</th><th className="py-2 font-medium">Date</th>
                </tr></thead>
                <tbody className="divide-y divide-ink/[0.06]">
                  {runs.map((run) => {
                    const done = run.status === 'succeeded';
                    return (
                      <tr key={run.id} onClick={() => setOpenRun(run.id)} aria-selected={openRun === run.id}
                        className={`cursor-pointer transition hover:bg-ink/[0.03] ${openRun === run.id ? 'bg-accent/[0.06]' : ''}`}>
                        <td className="py-3 pr-3">
                          <button type="button" onClick={(event) => { event.stopPropagation(); setOpenRun(run.id); }} className="text-left">
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
                        <td className="py-3 text-ink/50">{new Date(run.recorded_ts / 1e6).toLocaleDateString()}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              {!runs.length && <p className="py-6 text-center text-sm text-ink/45">No backtest runs recorded for this experiment yet.</p>}
              {nextCursor && (
                <button type="button" onClick={() => void loadRuns(selected, nextCursor).catch((cause) => setError(cause instanceof Error ? cause.message : 'Could not load more runs.'))}
                  className="btn-secondary mt-3 w-full text-xs">Load more runs</button>
              )}
            </div>
          </div>

          {openRun && (
            <RunDetail
              runId={openRun}
              experimentName={selectedExperiment?.name ?? 'Unsorted'}
              onClose={() => setOpenRun('')}
              onChanged={() => { if (selected) void loadRuns(selected).catch(() => undefined); }}
            />
          )}
        </section>
      </div>
    </div>
  );
}
