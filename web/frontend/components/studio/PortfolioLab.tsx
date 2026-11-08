'use client';

import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { Briefcase, FileDown, Pencil, Play, Plus, RefreshCw, Trash2, X } from 'lucide-react';
import { listDatasets, type DatasetVersion } from '@/lib/backtests';
import {
  canEditPortfolios,
  createPortfolio,
  deletePortfolio,
  downloadPortfolioReport,
  equitySeries,
  getPortfolio,
  getPortfolioRun,
  listPortfolioRuns,
  listPortfolios,
  listState,
  runPortfolio,
  runState,
  runSymbols,
  updatePortfolio,
  weightTotal,
  type CalendarFrequency,
  type HoldingInput,
  type PanelState,
  type Portfolio,
  type PortfolioMethod,
  type PortfolioRun,
  type PortfolioRunListItem,
  type PortfolioSummary,
  type RunPortfolioRequest,
} from '@/lib/portfolios';
import { ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import { EquityChart } from './backtests/Charts';
import { Field, NumberInput } from './backtests/Fields';
import { cash, pct } from './backtests/format';
import { PortfolioRunReportPanel } from './ReportPanel';

export const NO_PORTFOLIOS = 'No saved portfolios yet.';
export const NO_RUNS = 'No runs yet.';
export const NO_REBALANCES = 'This run made no rebalances.';
export const RUNNING = 'Running the portfolio backtest…';
export const READ_ONLY = 'You have view access: you can read portfolios and runs, but not change or run them.';

const METHODS: { id: PortfolioMethod; label: string }[] = [
  { id: 'fixed', label: 'Fixed weights' },
  { id: 'equal', label: 'Equal weight' },
  { id: 'inverse_volatility', label: 'Inverse volatility' },
  { id: 'risk_parity', label: 'Risk parity' },
];

const METHOD_HINT: Record<PortfolioMethod, string> = {
  fixed: 'Weights must add up to 100%.',
  equal: 'Each holding gets 1/N.',
  inverse_volatility: 'Weights proportional to 1 / annualized volatility.',
  risk_parity: 'Equal risk contributions, from the covariance of daily closes in a dataset version up to the as-of date (at least 60 closes each).',
};

/** The message for a panel that is not showing data: loading, error, running or empty. */
function StateNote({ state, error, empty }: { state: PanelState; error?: string | null; empty: string }) {
  if (state === 'loading')
    return (
      <p className="flex items-center gap-2 text-xs text-ink/50">
        <Spinner /> Loading…
      </p>
    );
  if (state === 'error') return <ErrorNote>{error}</ErrorNote>;
  if (state === 'running')
    return (
      <p className="flex items-center gap-2 text-xs text-accent">
        <Spinner /> {RUNNING}
      </p>
    );
  if (state === 'empty') return <p className="text-xs text-ink/50">{empty}</p>;
  return null;
}

function Panel({ testId, state, title, actions, children }: { testId: string; state: PanelState; title: ReactNode; actions?: ReactNode; children: ReactNode }) {
  return (
    <GlassCard className="space-y-3 p-5" data-testid={testId} data-state={state}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="flex items-center gap-2 text-sm font-semibold text-ink">{title}</h3>
        {actions}
      </div>
      {children}
    </GlassCard>
  );
}

const errorText = (e: unknown, fallback: string) => (e instanceof Error ? e.message : fallback);

/**
 * Studio → Portfolio → Saved portfolios (M15): list, create, edit and delete saved portfolios, run
 * a portfolio backtest on a dataset version, and read its weights over time, equity curve,
 * turnover and costs per rebalance, and stored reports. Viewers see everything read-only.
 */
export function PortfolioLab() {
  const [canEdit, setCanEdit] = useState(false);
  const [portfolios, setPortfolios] = useState<PortfolioSummary[] | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [editing, setEditing] = useState<'new' | Portfolio | null>(null);
  const [refresh, setRefresh] = useState(0);
  const load = useCallback(() => setRefresh((n) => n + 1), []);

  useEffect(() => {
    let live = true;
    listPortfolios()
      .then((list) => {
        if (!live) return;
        setPortfolios(list);
        setListError(null);
      })
      .catch((e: unknown) => {
        if (live) setListError(errorText(e, 'Could not list portfolios.'));
      });
    return () => {
      live = false;
    };
  }, [refresh]);

  useEffect(() => {
    fetch('/api/auth/session', { credentials: 'same-origin' })
      .then((r) => r.json())
      .then((s: { workspace?: { role?: string } | null }) => setCanEdit(canEditPortfolios(s.workspace?.role)))
      .catch(() => setCanEdit(false));
  }, []);

  const state = listState(portfolios, listError);
  return (
    <div className="space-y-5">
      {!canEdit && (
        <p className="text-xs text-ink/55" data-testid="portfolio-read-only">
          {READ_ONLY}
        </p>
      )}
      <Panel
        testId="portfolio-list"
        state={state}
        title={
          <>
            <Briefcase className="h-4 w-4 text-accent" /> Saved portfolios
          </>
        }
        actions={
          <div className="flex items-center gap-2">
            <button type="button" className="btn-ghost" onClick={load} title="Reload">
              <RefreshCw className="h-4 w-4" />
            </button>
            {canEdit && (
              <button type="button" className="btn-primary px-3 py-1.5 text-xs" onClick={() => setEditing('new')}>
                <Plus className="mr-1 inline h-3.5 w-3.5" /> New portfolio
              </button>
            )}
          </div>
        }
      >
        <StateNote state={state} error={listError} empty={NO_PORTFOLIOS} />
        {state === 'loaded' && portfolios && (
          <ul className="divide-y divide-ink/[0.06]">
            {portfolios.map((p) => (
              <li key={p.id}>
                <button
                  type="button"
                  onClick={() => setSelected(p.id)}
                  className={`flex w-full items-center justify-between px-2 py-2 text-left text-sm ${selected === p.id ? 'bg-accent/10 text-accent' : 'text-ink hover:bg-ink/[0.03]'}`}
                >
                  <span className="font-medium">{p.name}</span>
                  <span className="text-xs text-ink/50">{METHODS.find((m) => m.id === p.method)?.label ?? p.method}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </Panel>

      {editing && canEdit && (
        <PortfolioEditor
          initial={editing === 'new' ? null : editing}
          onCancel={() => setEditing(null)}
          onSaved={(p) => {
            setEditing(null);
            setSelected(p.id);
            load();
          }}
        />
      )}

      {selected && (
        <PortfolioDetail
          key={selected}
          portfolioId={selected}
          canEdit={canEdit}
          onEdit={(p) => setEditing(p)}
          onDeleted={() => {
            setSelected(null);
            load();
          }}
        />
      )}
    </div>
  );
}

/** Create or edit form: name, method and holdings, checked by the API with the same rules. */
function PortfolioEditor({ initial, onCancel, onSaved }: { initial: Portfolio | null; onCancel: () => void; onSaved: (p: Portfolio) => void }) {
  const [name, setName] = useState(initial?.name ?? '');
  const [method, setMethod] = useState<PortfolioMethod>(initial?.method ?? 'fixed');
  const [rows, setRows] = useState<HoldingInput[]>(
    initial ? initial.holdings.map((h) => ({ symbol: h.symbol, weight: h.weight })) : [{ symbol: '', weight: undefined }],
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [datasets, setDatasets] = useState<DatasetVersion[] | null>(null);
  const [datasetError, setDatasetError] = useState<string | null>(null);
  const [datasetId, setDatasetId] = useState('');
  const [asOf, setAsOf] = useState(today());
  const total = weightTotal(rows);
  const fromPrices = method === 'risk_parity';

  // Risk parity reads closes from a dataset version: list them the first time it is picked.
  useEffect(() => {
    if (!fromPrices || datasets !== null) return;
    listDatasets()
      .then((d) => {
        setDatasets(d);
        if (d[0]) setDatasetId((cur) => cur || d[0].id);
      })
      .catch((e: unknown) => setDatasetError(errorText(e, 'Could not list dataset versions.')));
  }, [fromPrices, datasets]);

  const set = (i: number, patch: Partial<HoldingInput>) => setRows((r) => r.map((row, k) => (k === i ? { ...row, ...patch } : row)));
  const save = async () => {
    setBusy(true);
    setError(null);
    const holdings = rows.map((r) => ({
      symbol: r.symbol.trim(),
      ...(method === 'fixed' ? { weight: r.weight } : {}),
      ...(method === 'inverse_volatility' ? { volatility: r.volatility } : {}),
    }));
    try {
      const body = { name: name.trim(), method, holdings, ...(fromPrices ? { dataset_version_id: datasetId, as_of: asOf } : {}) };
      onSaved(initial ? await updatePortfolio(initial.id, body) : await createPortfolio(body));
    } catch (e) {
      setError(errorText(e, 'Could not save the portfolio.'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Panel testId="portfolio-editor" state="loaded" title={initial ? `Edit ${initial.name}` : 'New portfolio'} actions={<button type="button" className="btn-ghost" onClick={onCancel} aria-label="Close"><X className="h-4 w-4" /></button>}>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="name">
          <input className="input py-1.5 text-[13px]" value={name} maxLength={120} onChange={(e) => setName(e.target.value)} />
        </Field>
        <Field label="method" hint={METHOD_HINT[method]}>
          <select className="input py-1.5 text-[13px]" value={method} onChange={(e) => setMethod(e.target.value as PortfolioMethod)}>
            {METHODS.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label}
              </option>
            ))}
          </select>
        </Field>
        {fromPrices && (
          <>
            <Field label="dataset version" hint="The daily closes the covariance is estimated from.">
              <select className="input py-1.5 text-[13px]" value={datasetId} onChange={(e) => setDatasetId(e.target.value)} disabled={!datasets?.length}>
                {(datasets ?? []).map((d) => (
                  <option key={d.id} value={d.id}>
                    {d.dataset_name} v{d.version}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="as of" hint="Only closes on or before this day are used.">
              <input type="date" className="input py-1.5 text-[13px]" value={asOf} max={today()} onChange={(e) => setAsOf(e.target.value)} />
            </Field>
          </>
        )}
      </div>
      {fromPrices && datasetError && <ErrorNote>{datasetError}</ErrorNote>}
      {fromPrices && datasets && datasets.length === 0 && <p className="text-xs text-ink/50">Add a dataset in the Data tab to build risk parity weights.</p>}
      <div className="space-y-2">
        {rows.map((r, i) => (
          <div key={i} className="flex items-end gap-2">
            <Field label="symbol" className="flex-1">
              <input className="input py-1.5 text-[13px] uppercase" value={r.symbol} onChange={(e) => set(i, { symbol: e.target.value.toUpperCase() })} />
            </Field>
            {method === 'fixed' && (
              <Field label="weight (fraction)" className="w-36">
                <NumberInput value={r.weight} min={0} max={1} onChange={(v) => set(i, { weight: v })} ariaLabel={`Weight of holding ${i + 1}`} />
              </Field>
            )}
            {method === 'inverse_volatility' && (
              <Field label="volatility" className="w-36">
                <NumberInput value={r.volatility} min={0} onChange={(v) => set(i, { volatility: v })} ariaLabel={`Volatility of holding ${i + 1}`} />
              </Field>
            )}
            <button type="button" className="btn-ghost mb-0.5" aria-label={`Remove holding ${i + 1}`} onClick={() => setRows((rs) => rs.filter((_, k) => k !== i))} disabled={rows.length === 1}>
              <Trash2 className="h-4 w-4" />
            </button>
          </div>
        ))}
        <div className="flex items-center justify-between text-xs">
          <button type="button" className="btn-secondary px-3 py-1.5 text-xs" onClick={() => setRows((rs) => [...rs, { symbol: '' }])}>
            <Plus className="mr-1 inline h-3.5 w-3.5" /> Add holding
          </button>
          {method === 'fixed' && <span className={total.ok ? 'text-ink/55' : 'text-loss'}>Total {pct(total.total)}</span>}
        </div>
      </div>
      {error && <ErrorNote>{error}</ErrorNote>}
      <div className="flex justify-end gap-2">
        <button type="button" className="btn-secondary px-3 py-1.5 text-xs" onClick={onCancel} disabled={busy}>
          Cancel
        </button>
        <button type="button" className="btn-primary px-3 py-1.5 text-xs" onClick={save} disabled={busy || !name.trim() || (fromPrices && !datasetId)}>
          {busy ? 'Saving…' : 'Save'}
        </button>
      </div>
    </Panel>
  );
}

/** One portfolio: its holdings, the run form, its runs and the selected run's result. */
function PortfolioDetail({ portfolioId, canEdit, onEdit, onDeleted }: { portfolioId: string; canEdit: boolean; onEdit: (p: Portfolio) => void; onDeleted: () => void }) {
  const [portfolio, setPortfolio] = useState<Portfolio | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [runs, setRuns] = useState<PortfolioRunListItem[] | null>(null);
  const [runsError, setRunsError] = useState<string | null>(null);
  const [run, setRun] = useState<PortfolioRun | null>(null);
  const [runError, setRunError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [selectedRun, setSelectedRun] = useState<string | null>(null);
  const [runsRefresh, setRunsRefresh] = useState(0);
  const loadRuns = useCallback(() => setRunsRefresh((n) => n + 1), []);

  useEffect(() => {
    getPortfolio(portfolioId)
      .then(setPortfolio)
      .catch((e: unknown) => setError(errorText(e, 'Could not load the portfolio.')));
  }, [portfolioId]);

  useEffect(() => {
    let live = true;
    listPortfolioRuns(portfolioId)
      .then((page) => {
        if (!live) return;
        setRuns(page.items);
        setRunsError(null);
      })
      .catch((e: unknown) => {
        if (live) setRunsError(errorText(e, 'Could not list runs.'));
      });
    return () => {
      live = false;
    };
  }, [portfolioId, runsRefresh]);

  const start = async (body: RunPortfolioRequest) => {
    setStarting(true);
    setRunError(null);
    try {
      const r = await runPortfolio(portfolioId, body);
      setRun(r);
      setSelectedRun(r.id);
    } catch (e) {
      setRun(null);
      setRunError(errorText(e, 'The run failed.'));
    } finally {
      setStarting(false);
      loadRuns();
    }
  };

  const remove = async () => {
    if (!portfolio || !window.confirm(`Delete ${portfolio.name}? Its runs are kept.`)) return;
    try {
      await deletePortfolio(portfolio.id);
      onDeleted();
    } catch (e) {
      setError(errorText(e, 'Could not delete the portfolio.'));
    }
  };

  const state: PanelState = error ? 'error' : portfolio ? 'loaded' : 'loading';
  return (
    <div className="space-y-5">
      <Panel
        testId="portfolio-detail"
        state={state}
        title={portfolio?.name ?? 'Portfolio'}
        actions={
          canEdit && portfolio ? (
            <div className="flex gap-2">
              <button type="button" className="btn-secondary px-3 py-1.5 text-xs" onClick={() => onEdit(portfolio)}>
                <Pencil className="mr-1 inline h-3.5 w-3.5" /> Edit
              </button>
              <button type="button" className="btn-secondary px-3 py-1.5 text-xs text-loss" onClick={remove}>
                <Trash2 className="mr-1 inline h-3.5 w-3.5" /> Delete
              </button>
            </div>
          ) : null
        }
      >
        <StateNote state={state} error={error} empty="" />
        {portfolio && (
          <table className="w-full text-xs">
            <thead>
              <tr className="text-left text-[11px] uppercase text-ink/50">
                <th className="py-1.5 font-medium">Symbol</th>
                <th className="py-1.5 text-right font-medium">Target weight</th>
              </tr>
            </thead>
            <tbody className="num">
              {portfolio.holdings.map((h) => (
                <tr key={h.symbol} className="border-t border-ink/[0.04]">
                  <td className="py-1.5 text-ink">{h.symbol}</td>
                  <td className="py-1.5 text-right text-ink">{pct(h.weight)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Panel>

      {canEdit && portfolio && <RunForm busy={starting} onRun={start} />}

      <Panel testId="portfolio-runs" state={listState(runs, runsError)} title="Runs">
        <StateNote state={listState(runs, runsError)} error={runsError} empty={NO_RUNS} />
        {runs && runs.length > 0 && (
          <table className="w-full text-xs">
            <thead>
              <tr className="text-left text-[11px] uppercase text-ink/50">
                <th className="py-1.5 font-medium">Started</th>
                <th className="py-1.5 font-medium">Status</th>
                <th className="py-1.5 text-right font-medium">Return</th>
                <th className="py-1.5 text-right font-medium">Cost</th>
              </tr>
            </thead>
            <tbody className="num">
              {runs.map((r) => (
                <tr
                  key={r.id}
                  className={`cursor-pointer border-t border-ink/[0.04] ${selectedRun === r.id ? 'bg-accent/10' : 'hover:bg-ink/[0.03]'}`}
                  onClick={() => setSelectedRun(r.id)}
                >
                  <td className="py-1.5 text-ink/70">{new Date(r.created_at).toLocaleString()}</td>
                  <td className={`py-1.5 ${r.status === 'failed' ? 'text-loss' : 'text-ink'}`} title={r.error ?? undefined}>
                    {r.status}
                  </td>
                  <td className="py-1.5 text-right text-ink">{r.headline ? pct(r.headline.total_return) : '—'}</td>
                  <td className="py-1.5 text-right text-ink">{r.headline ? cash(r.headline.total_cost) : '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Panel>

      {(starting || runError || selectedRun) && (
        <RunResult key={selectedRun ?? 'new'} runId={selectedRun} startedRun={run} starting={starting} startError={runError} canEdit={canEdit} />
      )}
    </div>
  );
}

const today = () => new Date().toISOString().slice(0, 10);

/** Dataset version, dates, rebalance rule, starting cash and cost config for a run. */
function RunForm({ busy, onRun }: { busy: boolean; onRun: (body: RunPortfolioRequest) => void }) {
  const [datasets, setDatasets] = useState<DatasetVersion[] | null>(null);
  const [datasetError, setDatasetError] = useState<string | null>(null);
  const [datasetId, setDatasetId] = useState('');
  const [startDate, setStartDate] = useState(`${new Date().getUTCFullYear() - 1}-01-01`);
  const [endDate, setEndDate] = useState(today());
  const [calendar, setCalendar] = useState<CalendarFrequency | ''>('monthly');
  const [drift, setDrift] = useState<number | undefined>(undefined);
  const [initialCash, setInitialCash] = useState<number | undefined>(100_000);
  const [costMode, setCostMode] = useState<'paper-default' | 'custom'>('paper-default');
  const [slippage, setSlippage] = useState<number | undefined>(0);
  const [minFee, setMinFee] = useState<number | undefined>(0);

  useEffect(() => {
    listDatasets()
      .then((d) => {
        setDatasets(d);
        if (d[0]) setDatasetId(d[0].id);
      })
      .catch((e: unknown) => setDatasetError(errorText(e, 'Could not list dataset versions.')));
  }, []);

  const ready = datasetId && initialCash !== undefined && (calendar || drift !== undefined);
  const submit = () => {
    if (!ready || initialCash === undefined) return;
    onRun({
      dataset_version_id: datasetId,
      start: startDate,
      end: endDate,
      rule: { calendar: calendar || null, drift_threshold: drift === undefined ? null : drift / 100 },
      initial_cash: initialCash,
      ...(costMode === 'custom' ? { cost_config: { slippage_bps: slippage ?? 0, min_fee: minFee ?? 0 } } : {}),
    });
  };

  return (
    <Panel testId="portfolio-run-form" state={listState(datasets, datasetError)} title="Run a portfolio backtest">
      {datasetError && <ErrorNote>{datasetError}</ErrorNote>}
      {datasets && datasets.length === 0 && <p className="text-xs text-ink/50">Add a dataset in the Data tab to run a backtest.</p>}
      <div className="grid gap-3 sm:grid-cols-3">
        <Field label="dataset version" className="sm:col-span-3">
          <select className="input py-1.5 text-[13px]" value={datasetId} onChange={(e) => setDatasetId(e.target.value)} disabled={!datasets?.length}>
            {(datasets ?? []).map((d) => (
              <option key={d.id} value={d.id}>
                {d.dataset_name} v{d.version}
              </option>
            ))}
          </select>
        </Field>
        <Field label="start">
          <input type="date" className="input py-1.5 text-[13px]" value={startDate} onChange={(e) => setStartDate(e.target.value)} />
        </Field>
        <Field label="end">
          <input type="date" className="input py-1.5 text-[13px]" value={endDate} onChange={(e) => setEndDate(e.target.value)} />
        </Field>
        <Field label="initial cash">
          <NumberInput value={initialCash} min={1} onChange={setInitialCash} ariaLabel="Initial cash" />
        </Field>
        <Field label="calendar rebalance">
          <select className="input py-1.5 text-[13px]" value={calendar} onChange={(e) => setCalendar(e.target.value as CalendarFrequency | '')}>
            <option value="monthly">Monthly</option>
            <option value="quarterly">Quarterly</option>
            <option value="">None</option>
          </select>
        </Field>
        <Field label="drift threshold (%)" hint="Rebalance when a weight moves this far from target. Empty: off.">
          <div className="flex gap-1">
            <NumberInput value={drift} min={0.01} max={100} onChange={setDrift} ariaLabel="Drift threshold in percent" />
            {drift !== undefined && (
              <button type="button" className="btn-ghost" onClick={() => setDrift(undefined)} aria-label="Turn off the drift threshold">
                <X className="h-4 w-4" />
              </button>
            )}
          </div>
        </Field>
        <Field label="cost config" hint={costMode === 'paper-default' ? 'M26 paper-default: 5 bps slippage, 1.00 fee per trade.' : undefined}>
          <select className="input py-1.5 text-[13px]" value={costMode} onChange={(e) => setCostMode(e.target.value as 'paper-default' | 'custom')}>
            <option value="paper-default">paper-default</option>
            <option value="custom">Custom</option>
          </select>
        </Field>
        {costMode === 'custom' && (
          <>
            <Field label="slippage (bps)">
              <NumberInput value={slippage} min={0} onChange={setSlippage} ariaLabel="Slippage in basis points" />
            </Field>
            <Field label="fee per trade">
              <NumberInput value={minFee} min={0} onChange={setMinFee} ariaLabel="Fee per trade" />
            </Field>
          </>
        )}
      </div>
      <div className="flex justify-end">
        <button type="button" className="btn-primary px-3 py-1.5 text-xs" onClick={submit} disabled={busy || !ready}>
          {busy ? <Spinner className="mr-1 inline h-3.5 w-3.5" /> : <Play className="mr-1 inline h-3.5 w-3.5" />}
          {busy ? 'Running…' : 'Run'}
        </button>
      </div>
    </Panel>
  );
}

/** A run's result: headline numbers, equity curve, weights over time, turnover and cost per rebalance, and stored reports. */
function RunResult({ runId, startedRun, starting, startError, canEdit }: { runId: string | null; startedRun: PortfolioRun | null; starting: boolean; startError: string | null; canEdit: boolean }) {
  const [run, setRun] = useState<PortfolioRun | null>(startedRun && startedRun.id === runId ? startedRun : null);
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [reportsRefresh, setReportsRefresh] = useState(0);

  useEffect(() => {
    if (!runId || (run && run.id === runId)) return;
    getPortfolioRun(runId)
      .then(setRun)
      .catch((e: unknown) => setError(errorText(e, 'Could not load the run.')));
  }, [runId, run]);

  const state = runState(run, startError ?? error, starting);
  const result = run?.result ?? null;
  const symbols = useMemo(() => (result ? runSymbols(result) : []), [result]);
  const series = useMemo(() => (result ? equitySeries(result) : null), [result]);

  const exportReport = () => {
    if (!run) return;
    setExporting(true);
    downloadPortfolioReport(run.id)
      .catch((e: unknown) => setError(errorText(e, 'Could not create the report.')))
      .finally(() => {
        setExporting(false);
        setReportsRefresh((n) => n + 1);
      });
  };

  return (
    <div className="space-y-5">
      <Panel
        testId="portfolio-run-result"
        state={state}
        title="Run result"
        actions={
          canEdit && run?.status === 'succeeded' ? (
            <button type="button" className="btn-secondary px-3 py-1.5 text-xs" onClick={exportReport} disabled={exporting}>
              <FileDown className="mr-1 inline h-3.5 w-3.5" /> {exporting ? 'Creating…' : 'Report PDF'}
            </button>
          ) : null
        }
      >
        <StateNote state={state} error={startError ?? error ?? run?.error} empty={NO_REBALANCES} />
        {result && (
          <>
            <dl className="grid grid-cols-2 gap-3 text-xs sm:grid-cols-4">
              {[
                ['Total return', pct(result.summary.total_return)],
                ['Max drawdown', pct(result.summary.max_drawdown)],
                ['Final equity', cash(result.summary.final_equity)],
                ['Rebalances', String(result.summary.rebalance_count)],
                ['Total turnover', pct(result.summary.total_turnover)],
                ['Total cost', cash(result.summary.total_cost)],
                ['Cost config', `${result.cost_config.name} v${result.cost_config.version}`],
                ['Days priced', `${result.prices.days} (${result.prices.skipped_days} skipped)`],
              ].map(([label, value]) => (
                <div key={label}>
                  <dt className="text-ink/50">{label}</dt>
                  <dd className="num font-semibold text-ink">{value}</dd>
                </div>
              ))}
            </dl>
            <EquityChart series={series} start={result.summary.initial_cash} height={220} />
          </>
        )}
      </Panel>

      {result && result.rebalances.length > 0 && (
        <Panel testId="portfolio-weights" state="loaded" title="Weights at each rebalance">
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead>
                <tr className="text-left text-[11px] uppercase text-ink/50">
                  <th className="py-1.5 font-medium">Date</th>
                  {symbols.map((s) => (
                    <th key={s} className="py-1.5 text-right font-medium">
                      {s} before → after
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="num">
                {result.rebalances.map((r) => (
                  <tr key={r.date} className="border-t border-ink/[0.04]">
                    <td className="py-1.5 text-ink/70">{r.date}</td>
                    {symbols.map((s) => {
                      const w = r.weights.find((x) => x.symbol === s);
                      return (
                        <td key={s} className="py-1.5 text-right text-ink">
                          {w ? `${pct(w.before)} → ${pct(w.after)}` : '—'}
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Panel>
      )}

      {result && result.rebalances.length > 0 && (
        <Panel testId="portfolio-rebalances" state="loaded" title="Turnover and cost per rebalance">
          <table className="w-full text-xs">
            <thead>
              <tr className="text-left text-[11px] uppercase text-ink/50">
                <th className="py-1.5 font-medium">Date</th>
                <th className="py-1.5 font-medium">Reason</th>
                <th className="py-1.5 text-right font-medium">Trades</th>
                <th className="py-1.5 text-right font-medium">Turnover</th>
                <th className="py-1.5 text-right font-medium">Cost</th>
              </tr>
            </thead>
            <tbody className="num">
              {result.rebalances.map((r) => (
                <tr key={r.date} className="border-t border-ink/[0.04]">
                  <td className="py-1.5 text-ink/70">{r.date}</td>
                  <td className="py-1.5 text-ink">{r.reasons.join(', ')}</td>
                  <td className="py-1.5 text-right text-ink">{r.trades.length}</td>
                  <td className="py-1.5 text-right text-ink">{pct(r.turnover)}</td>
                  <td className="py-1.5 text-right text-ink">{cash(r.cost)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Panel>
      )}

      {run?.status === 'succeeded' && <PortfolioRunReportPanel runId={run.id} refreshKey={reportsRefresh} />}
    </div>
  );
}
