'use client';

import { useEffect, useMemo, useState } from 'react';
import { ChevronDown, ChevronUp, Database, Layers, Play, Plus, SlidersHorizontal, Trash2 } from 'lucide-react';
import { ApiError } from '@/lib/api';
import {
  createBacktest,
  defaultParams,
  fieldErrors,
  MAX_STRATEGIES,
  type Backtest,
  type Catalog,
  type CreateBacktest,
  type DatasetVersion,
  type ParamValues,
  type StoredRequest,
  type UserConfig,
} from '@/lib/backtests';
import { Badge, ErrorNote, GlassCard, Segmented, Spinner } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';
import { ConfigFields, Field, NumberInput, ParamFields } from './Fields';
import { int } from './format';

type Slot = { key: number; strategy: string; params: ParamValues; weightPct: number };

/** A request to load into the form ("edit and run again"); `nonce` makes the same one load twice. */
export type Prefill = { request: StoredRequest; nonce: number };

let nextKey = 1;

function slotFor(catalog: Catalog, name: string, weightPct: number, params?: ParamValues): Slot {
  const s = catalog.strategies.find((x) => x.name === name) ?? catalog.strategies[0];
  return { key: nextKey++, strategy: s.name, params: { ...defaultParams(s), ...(params ?? {}) }, weightPct };
}

/** Rounds away float noise from weights entered as percentages (33.3 → 0.333). */
const toWeight = (pct: number) => Math.round(pct * 1e6) / 1e8;

export function BacktestForm({ catalog, datasets, canRun, onCreated, prefill }: { catalog: Catalog; datasets: DatasetVersion[]; canRun: boolean; onCreated: (b: Backtest) => void; prefill?: Prefill | null }) {
  const first = catalog.strategies[0]?.name ?? '';
  const [datasetId, setDatasetId] = useState(datasets[0]?.id ?? '');
  const [mode, setMode] = useState<'single' | 'multi'>('single');
  const [strategy, setStrategy] = useState(first);
  const [params, setParams] = useState<ParamValues>(() => {
    const s = catalog.strategies.find((x) => x.name === first);
    return s ? defaultParams(s) : {};
  });
  const [slots, setSlots] = useState<Slot[]>(() =>
    catalog.strategies.length > 1
      ? [slotFor(catalog, catalog.strategies[0].name, 50), slotFor(catalog, catalog.strategies[1].name, 50)]
      : [slotFor(catalog, first, 100)],
  );
  const [openSlot, setOpenSlot] = useState<number | null>(null);
  const [preset, setPreset] = useState(catalog.presets[0]?.name ?? 'default');
  const [overrides, setOverrides] = useState<UserConfig>({});
  const [showConfig, setShowConfig] = useState(false);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  // Keep the dataset choice valid as the list loads or changes.
  useEffect(() => {
    if (!datasets.some((d) => d.id === datasetId)) setDatasetId(datasets[0]?.id ?? '');
  }, [datasets, datasetId]);

  // "Edit and run again": load a stored request.
  useEffect(() => {
    if (!prefill) return;
    const r = prefill.request;
    if (r.dataset_version_id && datasets.some((d) => d.id === r.dataset_version_id)) setDatasetId(r.dataset_version_id);
    if (r.strategies?.length) {
      setMode('multi');
      setSlots(r.strategies.map((s) => slotFor(catalog, s.strategy, Math.round(s.weight * 1e6) / 1e4, s.params)));
    } else if (r.strategy) {
      const s = catalog.strategies.find((x) => x.name === r.strategy);
      if (s) {
        setMode('single');
        setStrategy(s.name);
        setParams({ ...defaultParams(s), ...(r.params ?? {}) });
      }
    }
    const { preset: p, ...rest } = r.config ?? {};
    const name = typeof p === 'string' && catalog.presets.some((x) => x.name === p) ? p : catalog.presets[0]?.name ?? 'default';
    setPreset(name);
    const base = catalog.presets.find((x) => x.name === name)?.config ?? {};
    setOverrides(Object.fromEntries(Object.entries(rest).filter(([k, v]) => base[k] !== v)));
    setErrors({});
    setError('');
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reload only for a new prefill
  }, [prefill?.nonce]);

  const base = catalog.presets.find((p) => p.name === preset)?.config ?? {};
  const current = catalog.strategies.find((s) => s.name === strategy);
  const totalPct = slots.reduce((a, s) => a + (Number.isFinite(s.weightPct) ? s.weightPct : 0), 0);
  const overAllocated = toWeight(totalPct) > 1 + 1e-9;
  const dataset = datasets.find((d) => d.id === datasetId);
  const changed = Object.keys(overrides).length;
  const configErrors = Object.keys(errors).filter((k) => k.startsWith('config.')).length;

  const updateSlot = (key: number, patch: Partial<Slot>) => setSlots((ss) => ss.map((s) => (s.key === key ? { ...s, ...patch } : s)));

  const body = useMemo((): CreateBacktest | null => {
    if (!datasetId) return null;
    const config: UserConfig = { preset, ...overrides };
    if (mode === 'single') return { dataset_version_id: datasetId, strategy, params, config };
    return {
      dataset_version_id: datasetId,
      strategies: slots.map((s) => ({ strategy: s.strategy, params: s.params, weight: toWeight(s.weightPct) })),
      config,
    };
  }, [datasetId, mode, strategy, params, slots, preset, overrides]);

  const submit = async () => {
    if (!body) return;
    setBusy(true);
    setErrors({});
    setError('');
    try {
      onCreated(await createBacktest(body));
    } catch (e) {
      if (e instanceof ApiError && e.status === 422) {
        const f = fieldErrors(e.message);
        setErrors(f);
        if (Object.keys(f).some((k) => k.startsWith('config.'))) setShowConfig(true);
        const shown = (k: string) => k.startsWith('config.') || /^params\./.test(k) || /^strategies\[\d+\]\.(params\.|weight|strategy)/.test(k);
        const rest = Object.entries(f).filter(([k]) => !shown(k));
        setError(rest.length ? rest.map(([k, m]) => (k ? `${k}: ${m}` : m)).join(' · ') : 'Some fields need fixing; see the messages next to them.');
        const bad = Object.keys(f).map((k) => /^strategies\[(\d+)\]/.exec(k)).find(Boolean);
        if (bad) setOpenSlot(slots[Number(bad[1])]?.key ?? null);
      } else if (e instanceof ApiError && e.status === 403) {
        setError('Running a backtest needs the editor role in this workspace.');
      } else {
        setError(e instanceof Error ? e.message : 'Could not queue the backtest.');
      }
    } finally {
      setBusy(false);
    }
  };

  const disabled = !canRun || busy;

  return (
    <GlassCard as="section" className="p-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold text-ink">New backtest</h2>
          <p className="mt-0.5 text-xs text-ink/50">Runs on a worker; results are stored with fingerprints you can reproduce from the CLI.</p>
        </div>
        <Segmented
          label="backtest-mode"
          size="sm"
          value={mode}
          onChange={setMode}
          options={[
            { value: 'single', label: 'One strategy' },
            { value: 'multi', label: 'Several strategies' },
          ]}
        />
      </div>

      <div className="mt-4 space-y-5">
        {/* Dataset */}
        <Field label="dataset" error={errors.dataset_version_id} hint={dataset ? `${int(dataset.row_count)} bars · ${dataset.checksum.slice(0, 12)}…` : undefined}>
          {datasets.length ? (
            <div className="relative">
              <Database className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-ink/40" />
              <select className="input py-1.5 pl-8 text-[13px]" value={datasetId} onChange={(e) => setDatasetId(e.target.value)} disabled={disabled} aria-label="Dataset version">
                {datasets.map((d) => (
                  <option key={d.id} value={d.id}>
                    {d.dataset_name} · v{d.version} · {int(d.row_count)} bars
                  </option>
                ))}
              </select>
            </div>
          ) : (
            <div className="rounded-lg border border-dashed border-ink/15 p-3 text-[11.5px] leading-5 text-ink/55">
              No datasets in this workspace yet.{' '}
              <a className="text-accent hover:underline" href="/studio?tab=data">
                Upload a CSV in the Data tab
              </a>
              , then come back to run a backtest on it.
            </div>
          )}
        </Field>

        {/* Strategy or strategies */}
        {mode === 'single' ? (
          <div className="space-y-3">
            <Field label="strategy" error={errors.strategy} hint={current?.description}>
              <select
                className="input py-1.5 text-[13px]"
                aria-label="Strategy"
                value={strategy}
                disabled={disabled}
                onChange={(e) => {
                  const s = catalog.strategies.find((x) => x.name === e.target.value);
                  if (!s) return;
                  setStrategy(s.name);
                  setParams(defaultParams(s));
                  setErrors({});
                }}
              >
                {catalog.strategies.map((s) => (
                  <option key={s.name} value={s.name}>
                    {s.label}
                  </option>
                ))}
              </select>
            </Field>
            {current && <ParamFields strategy={current} values={params} onChange={setParams} errors={errors} prefix="params." disabled={disabled} />}
          </div>
        ) : (
          <div className="space-y-2.5">
            <div className="flex items-center justify-between">
              <p className="flex items-center gap-1.5 text-[11.5px] font-medium text-ink/60">
                <Layers className="h-3.5 w-3.5" /> Strategies on one account
              </p>
              <span className={cn('num text-[11px]', overAllocated ? 'text-loss' : 'text-ink/50')}>
                {totalPct.toFixed(1)}% allocated{!overAllocated && totalPct < 100 ? ` · ${(100 - totalPct).toFixed(1)}% stays in cash` : ''}
              </span>
            </div>
            <div className="h-1.5 overflow-hidden rounded-full bg-ink/5" aria-hidden>
              <div className={cn('h-full rounded-full transition-[width]', overAllocated ? 'bg-loss' : 'bg-accent')} style={{ width: `${Math.min(100, totalPct)}%` }} />
            </div>
            {errors.strategies && <p className="text-[11px] text-loss">{errors.strategies}</p>}
            {slots.map((slot, i) => {
              const s = catalog.strategies.find((x) => x.name === slot.strategy);
              const open = openSlot === slot.key;
              const slotErrors = Object.keys(errors).some((k) => k.startsWith(`strategies[${i}].`));
              return (
                <div key={slot.key} className={cn('rounded-xl border p-3', slotErrors ? 'border-loss/40' : 'border-ink/[0.08] bg-ink/[0.015]')}>
                  <div className="grid grid-cols-[1fr_96px_auto_auto] items-end gap-2">
                    <Field label={`#${i + 1} strategy`} error={errors[`strategies[${i}].strategy`]}>
                      <select
                        className="input py-1.5 text-[13px]"
                        aria-label={`Strategy ${i + 1}`}
                        value={slot.strategy}
                        disabled={disabled}
                        onChange={(e) => {
                          const next = catalog.strategies.find((x) => x.name === e.target.value);
                          if (next) updateSlot(slot.key, { strategy: next.name, params: defaultParams(next) });
                        }}
                      >
                        {catalog.strategies.map((x) => (
                          <option key={x.name} value={x.name}>
                            {x.label}
                          </option>
                        ))}
                      </select>
                    </Field>
                    <Field label="weight %" error={errors[`strategies[${i}].weight`]}>
                      <NumberInput ariaLabel={`Weight of strategy ${i + 1}, percent`} value={slot.weightPct} min={0.01} max={100} disabled={disabled} onChange={(n) => updateSlot(slot.key, { weightPct: n })} />
                    </Field>
                    <button type="button" className="btn-ghost mb-0.5 h-8" onClick={() => setOpenSlot(open ? null : slot.key)} aria-expanded={open} aria-label={`Parameters of strategy ${i + 1}`}>
                      <SlidersHorizontal className="h-3.5 w-3.5" />
                      {open ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
                    </button>
                    <button
                      type="button"
                      className="btn-ghost mb-0.5 h-8 hover:text-loss"
                      disabled={disabled || slots.length === 1}
                      onClick={() => setSlots((ss) => ss.filter((x) => x.key !== slot.key))}
                      aria-label={`Remove strategy ${i + 1}`}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </button>
                  </div>
                  {open && s && (
                    <div className="mt-3 border-t border-ink/[0.06] pt-3">
                      <p className="mb-2.5 text-[11px] text-ink/45">{s.description}</p>
                      <ParamFields strategy={s} values={slot.params} onChange={(p) => updateSlot(slot.key, { params: p })} errors={errors} prefix={`strategies[${i}].params.`} disabled={disabled} />
                    </div>
                  )}
                </div>
              );
            })}
            <div className="flex flex-wrap items-center gap-2">
              <button type="button" className="btn-secondary px-3 py-1.5 text-xs" disabled={disabled || slots.length >= MAX_STRATEGIES} onClick={() => setSlots((ss) => [...ss, slotFor(catalog, first, Math.max(0, Math.round((100 - totalPct) * 100) / 100))])}>
                <Plus className="h-3.5 w-3.5" /> Add strategy
              </button>
              <button
                type="button"
                className="btn-ghost"
                disabled={disabled}
                onClick={() => {
                  const each = Math.floor(10_000 / slots.length) / 100;
                  setSlots((ss) => ss.map((s) => ({ ...s, weightPct: each })));
                }}
              >
                Split equally
              </button>
              <p className="text-[10.5px] text-ink/45">Each strategy trades on its own share of the cash and keeps its own books.</p>
            </div>
          </div>
        )}

        {/* Configuration */}
        <div className="space-y-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              <span className="text-[11.5px] font-medium text-ink/60">Costs preset</span>
              <Segmented label="backtest-preset" size="sm" value={preset} onChange={(p) => setPreset(p)} options={catalog.presets.map((p) => ({ value: p.name, label: p.label }))} />
            </div>
            <button type="button" className="btn-ghost" onClick={() => setShowConfig((v) => !v)} aria-expanded={showConfig}>
              <SlidersHorizontal className="h-3.5 w-3.5" />
              Settings
              {changed > 0 && <Badge tone="accent">{changed} changed</Badge>}
              {configErrors > 0 && <Badge tone="loss">{configErrors} to fix</Badge>}
              {showConfig ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
            </button>
          </div>
          {showConfig && (
            <div className="rounded-xl border border-ink/[0.07] bg-canvas/40 p-4">
              <ConfigFields schema={catalog.config_schema} base={base} overrides={overrides} onChange={setOverrides} errors={errors} disabled={disabled} />
            </div>
          )}
        </div>

        {error && <ErrorNote>{error}</ErrorNote>}

        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-ink/[0.06] pt-4">
          <p className="text-[11px] text-ink/45">
            {canRun ? (dataset ? `${mode === 'multi' ? `${slots.length} strategies` : current?.label ?? strategy} on ${dataset.dataset_name} v${dataset.version}` : 'Pick a dataset to run on.') : 'Viewers can look at runs; running one needs the editor role.'}
          </p>
          <button type="button" className="btn-primary px-4 py-2 text-xs" onClick={submit} disabled={disabled || !body || (mode === 'multi' && overAllocated)}>
            {busy ? <Spinner className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />}
            {busy ? 'Queuing…' : 'Run backtest'}
          </button>
        </div>
      </div>
    </GlassCard>
  );
}
