'use client';

import type { MetricId } from '@/lib/learn/metrics';
import { InfoTip, MetricLabel } from '@/components/ui/MetricInfo';
import { useEffect, useMemo, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { ArrowUpDown, CheckCircle2, ChevronDown, ChevronUp, ClipboardPaste, Download, FileUp, Plus, RotateCcw, Trash2 } from 'lucide-react';
import { calculateRisk } from '@/lib/api';
import { DEFAULT_VOLATILITY } from '@/lib/model';
import { positionValue } from '@/lib/portfolio';
import type { Position } from '@/lib/types';
import { cn, formatCurrency, formatNumber, formatPercent } from '@/lib/utils';
import { Badge, ErrorNote, GlassCard } from '@/components/ui/primitives';
import { PresetMenu } from './PresetMenu';
import { usePortfolio } from './PortfolioContext';

const PALETTE = ['#22D3EE', '#38BDF8', '#818CF8', '#34D399', '#F472B6', '#FBBF24', '#A78BFA', '#2DD4BF'];

type SortKey = 'symbol' | 'quantity' | 'price' | 'volatility' | 'value' | 'weight';

/** HHI bands on the 0–10,000 scale used in antitrust practice (DOJ/FTC 2023 guidelines). */
function hhiBand(hhi: number) {
  const pts = hhi * 10_000;
  if (pts > 1800) return { label: 'Highly concentrated', tone: 'loss' as const };
  if (pts >= 1000) return { label: 'Moderately concentrated', tone: 'caution' as const };
  return { label: 'Unconcentrated', tone: 'profit' as const };
}

function NumberCell({ value, onCommit, placeholder, decimals, className }: { value: number | undefined; onCommit: (v: number | undefined) => boolean; placeholder?: string; decimals?: number; className?: string }) {
  const fmt = (v: number | undefined) => (v === undefined ? '' : decimals !== undefined ? v.toFixed(decimals) : String(v));
  const [text, setText] = useState(fmt(value));
  const [bad, setBad] = useState(false);
  useEffect(() => {
    setText(value === undefined ? '' : decimals !== undefined ? value.toFixed(decimals) : String(value));
  }, [value, decimals]);
  const reject = () => {
    setBad(true);
    setText(fmt(value));
  };
  const commit = () => {
    const t = text.trim();
    if (t === '') return placeholder !== undefined && onCommit(undefined) ? setBad(false) : reject();
    const v = Number(t);
    return Number.isFinite(v) && onCommit(v) ? setBad(false) : reject();
  };
  return (
    <input
      value={text}
      placeholder={placeholder}
      inputMode="decimal"
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => e.key === 'Enter' && (e.currentTarget as HTMLInputElement).blur()}
      className={cn('num w-full rounded-md border border-transparent bg-transparent px-2 py-1 text-right text-[13px] text-ink/90 placeholder:text-ink/40 transition hover:border-ink/10 focus:border-accent/50 focus:bg-canvas focus:outline-none', bad && 'border-loss/60', className)}
    />
  );
}

export function PortfolioStudio() {
  const { positions, stats, csv, source, engine, setPositions, loadSample, loadPreset, importCsv, revision } = usePortfolio();
  const [sort, setSort] = useState<{ key: SortKey; dir: 1 | -1 }>({ key: 'value', dir: -1 });
  const [pasteOpen, setPasteOpen] = useState(false);
  const [pasteText, setPasteText] = useState('');
  const [errors, setErrors] = useState<string[]>([]);
  const [engineHhi, setEngineHhi] = useState<number | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const online = engine.status === 'online';

  // Cross-check the client-side HHI against the engine's (same formula, gross weights).
  useEffect(() => {
    if (!online || !positions.length) return setEngineHhi(null);
    let live = true;
    const t = setTimeout(() => {
      calculateRisk({ portfolio_csv: csv, metric: 'hhi', confidence: 0.99, simulations: 1_000, horizon_days: 1, backend: 'auto' })
        .then((r) => live && setEngineHhi(r.value))
        .catch(() => live && setEngineHhi(null));
    }, 800); // debounced so editing a position doesn't hit the engine on every keystroke
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [online, csv, revision, positions.length]);

  const rows = useMemo(() => {
    const withIdx = positions.map((p, i) => ({ p, i, value: positionValue(p), weight: stats.weights[i] ?? 0 }));
    const get = (r: (typeof withIdx)[number]) =>
      sort.key === 'symbol' ? r.p.symbol : sort.key === 'value' ? r.value : sort.key === 'weight' ? r.weight : sort.key === 'volatility' ? r.p.volatility ?? DEFAULT_VOLATILITY : r.p[sort.key];
    return withIdx.sort((a, b) => {
      const x = get(a);
      const y = get(b);
      return (typeof x === 'string' ? x.localeCompare(String(y)) : (x as number) - (y as number)) * sort.dir;
    });
  }, [positions, stats.weights, sort]);

  const update = (i: number, patch: Partial<Position>) => {
    const next = positions.map((p, j) => (j === i ? { ...p, ...patch } : p));
    if (patch.volatility === undefined && 'volatility' in patch) delete next[i].volatility;
    setPositions(next, 'edited');
  };
  const remove = (i: number) => setPositions(positions.filter((_, j) => j !== i), 'edited');
  const add = () => {
    let n = positions.length + 1;
    while (positions.some((p) => p.symbol === `NEW${n}`)) n++;
    setPositions([...positions, { symbol: `NEW${n}`, quantity: 100, price: 100 }], 'edited');
  };

  const onFile = async (f: File | undefined) => {
    if (!f) return;
    if (f.size > 10_000_000) return setErrors(['File exceeds the engine’s 10 MB limit.']);
    setErrors(importCsv(await f.text(), 'uploaded'));
  };
  const applyPaste = () => {
    const errs = importCsv(pasteText, 'pasted');
    setErrors(errs);
    if (!errs.length) setPasteOpen(false);
  };
  const download = () => {
    const url = URL.createObjectURL(new Blob([csv], { type: 'text/csv' }));
    const a = Object.assign(document.createElement('a'), { href: url, download: 'portfolio.csv' });
    a.click();
    URL.revokeObjectURL(url);
  };

  const band = hhiBand(stats.hhi);
  const hhiMatch = engineHhi !== null && Math.abs(engineHhi - stats.hhi) < 1e-9;
  const header = (key: SortKey, label: string, align: 'left' | 'right' = 'right', metric?: MetricId) => (
    <th className={cn('px-3 py-2.5 font-medium', align === 'right' ? 'text-right' : 'text-left')}>
      <button type="button" onClick={() => setSort((s) => ({ key, dir: s.key === key ? (s.dir === 1 ? -1 : 1) : key === 'symbol' ? 1 : -1 }))} className={cn('inline-flex items-center gap-1 transition hover:text-ink/90', sort.key === key && 'text-ink/90')}>
        {label}
        {sort.key === key ? sort.dir === 1 ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" /> : <ArrowUpDown className="h-3 w-3 opacity-40" />}
      </button>
      {metric && <InfoTip metric={metric} className="ml-1" />}
    </th>
  );

  return (
    <div className="space-y-5">
      {/* KPIs */}
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <GlassCard className="p-5">
          <p className="text-xs text-ink/60"><MetricLabel metric="portfolio_value">Net market value</MetricLabel></p>
          <p className="num mt-2 text-2xl font-semibold text-ink">{formatCurrency(stats.netValue)}</p>
          <p className="num mt-1 text-[11.5px] text-ink/50">{positions.length} positions</p>
        </GlassCard>
        <GlassCard className="p-5">
          <p className="text-xs text-ink/60"><MetricLabel metric="gross_exposure">Gross exposure</MetricLabel></p>
          <p className="num mt-2 text-2xl font-semibold text-ink">{formatCurrency(stats.grossExposure)}</p>
          <p className="num mt-1 text-[11.5px] text-ink/50">Σ |quantity × price|</p>
        </GlassCard>
        <GlassCard className="p-5">
          <div className="flex items-center justify-between">
            <p className="text-xs text-ink/60"><MetricLabel metric="hhi">HHI concentration</MetricLabel></p>
            <Badge tone={band.tone}>{band.label}</Badge>
          </div>
          <p className="num mt-2 text-2xl font-semibold text-ink">{formatNumber(stats.hhi, 4)}</p>
          <p className="num mt-1 flex items-center gap-1.5 text-[11.5px] text-ink/50">
            {formatNumber(stats.hhi * 10_000, 0)} pts · effective N {formatNumber(stats.effectiveN, 2)}
            {hhiMatch && (
              <span className="flex items-center gap-1 text-profit" title={`Engine HHI ${engineHhi}`}>
                <CheckCircle2 className="h-3 w-3" /> engine
              </span>
            )}
          </p>
        </GlassCard>
        <GlassCard className="p-5">
          <p className="text-xs text-ink/60"><MetricLabel metric="risk_volatility">Model volatility</MetricLabel></p>
          <p className="num mt-2 text-2xl font-semibold text-ink">{formatPercent(stats.modelVolatility)}</p>
          <p className="num mt-1 text-[11.5px] text-ink/50">average of asset σ · 20% default</p>
        </GlassCard>
      </div>

      {/* Weight strip */}
      <GlassCard className="p-5">
        <div className="mb-3 flex items-center justify-between text-xs">
          <span className="font-medium text-ink/75">Gross weight</span>
          <span className="text-ink/50">{source}</span>
        </div>
        <div className="flex h-3 overflow-hidden rounded-full bg-ink/5">
          {positions.map((p, i) => (
            <motion.div key={`${p.symbol}-${i}`} layout title={`${p.symbol} ${formatPercent(stats.weights[i] ?? 0)}`} className="h-full border-r border-canvas/60 last:border-0" style={{ width: `${(stats.weights[i] ?? 0) * 100}%`, background: PALETTE[i % PALETTE.length] }} />
          ))}
        </div>
        <div className="mt-3 flex flex-wrap gap-x-4 gap-y-1.5">
          {positions.map((p, i) => (
            <span key={`${p.symbol}-${i}`} className="flex items-center gap-1.5 text-[11.5px] text-ink/60">
              <i className="h-2 w-2 rounded-sm" style={{ background: PALETTE[i % PALETTE.length] }} />
              {p.symbol} <span className="num text-ink/50">{formatPercent(stats.weights[i] ?? 0, 1)}</span>
            </span>
          ))}
        </div>
      </GlassCard>

      {/* Table */}
      <GlassCard as="section" className="overflow-hidden">
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-ink/[0.06] px-5 py-4">
          <div>
            <h2 className="text-sm font-semibold text-ink">Positions</h2>
            <p className="mt-0.5 text-xs text-ink/50">Click a cell to edit. Leave volatility blank for the 20% default. Changes flow into the Risk Lab.</p>
          </div>
          <div className="flex flex-wrap gap-2">
            <button type="button" className="btn-secondary px-3 py-1.5 text-xs" onClick={() => { setErrors([]); loadSample(); }}>
              <RotateCcw className="h-3.5 w-3.5" /> Load sample
            </button>
            <PresetMenu onSelect={(id) => { setErrors([]); loadPreset(id); }} />
            <button type="button" className="btn-secondary px-3 py-1.5 text-xs" onClick={() => fileRef.current?.click()}>
              <FileUp className="h-3.5 w-3.5" /> Upload CSV
            </button>
            <input ref={fileRef} type="file" accept=".csv,text/csv,text/plain" className="hidden" onChange={(e) => { onFile(e.target.files?.[0]); e.target.value = ''; }} />
            <button type="button" className={cn('btn-secondary px-3 py-1.5 text-xs', pasteOpen && 'border-accent/40 text-accent')} onClick={() => { setPasteText(csv); setPasteOpen((o) => !o); }}>
              <ClipboardPaste className="h-3.5 w-3.5" /> Paste CSV
            </button>
            <button type="button" className="btn-secondary px-3 py-1.5 text-xs" onClick={download} aria-label="Download CSV">
              <Download className="h-3.5 w-3.5" />
            </button>
          </div>
        </div>

        <AnimatePresence>
          {pasteOpen && (
            <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} className="overflow-hidden border-b border-ink/[0.06]">
              <div className="space-y-3 p-5">
                <textarea value={pasteText} onChange={(e) => setPasteText(e.target.value)} spellCheck={false} rows={7} className="input font-mono text-xs leading-5" placeholder={'symbol,quantity,price,volatility\nAAPL,1000,190.00,0.28'} />
                <div className="flex justify-end gap-2">
                  <button type="button" className="btn-ghost" onClick={() => setPasteOpen(false)}>
                    Cancel
                  </button>
                  <button type="button" className="btn-primary px-3 py-1.5 text-xs" onClick={applyPaste}>
                    Apply CSV
                  </button>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>

        {errors.length > 0 && (
          <div className="border-b border-ink/[0.06] p-4">
            <ErrorNote>
              {errors.slice(0, 5).map((e) => (
                <p key={e}>{e}</p>
              ))}
              {errors.length > 5 && <p>…and {errors.length - 5} more</p>}
            </ErrorNote>
          </div>
        )}

        <div className="overflow-x-auto">
          <table className="w-full min-w-[760px] text-sm">
            <thead>
              <tr className="border-b border-ink/[0.06] text-[10.5px] uppercase tracking-wider text-ink/50">
                {header('symbol', 'Symbol', 'left')}
                {header('quantity', 'Quantity')}
                {header('price', 'Price')}
                {header('volatility', 'Volatility', 'right', 'annualized_volatility')}
                {header('value', 'Value')}
                {header('weight', 'Gross weight')}
                <th className="w-10" />
              </tr>
            </thead>
            <tbody>
              <AnimatePresence initial={false}>
                {rows.map(({ p, i, value, weight }) => (
                  <motion.tr key={`${p.symbol}-${i}`} layout initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="group border-b border-ink/[0.04] transition-colors hover:bg-ink/[0.02]">
                    <td className="px-3 py-1.5">
                      <div className="flex items-center gap-2">
                        <i className="h-2 w-2 shrink-0 rounded-sm" style={{ background: PALETTE[i % PALETTE.length] }} />
                        <input
                          defaultValue={p.symbol}
                          key={p.symbol}
                          onBlur={(e) => {
                            const v = e.target.value.trim().replace(/,/g, '');
                            if (v && v !== p.symbol) update(i, { symbol: v });
                            else e.target.value = p.symbol;
                          }}
                          onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
                          className="w-24 rounded-md border border-transparent bg-transparent px-1.5 py-1 font-mono text-[13px] font-medium text-ink transition hover:border-ink/10 focus:border-accent/50 focus:bg-canvas focus:outline-none"
                        />
                      </div>
                    </td>
                    <td className="w-32 px-1">
                      <NumberCell value={p.quantity} onCommit={(v) => v !== undefined && (update(i, { quantity: v }), true)} />
                    </td>
                    <td className="w-32 px-1">
                      <NumberCell value={p.price} decimals={2} onCommit={(v) => v !== undefined && v >= 0 && (update(i, { price: v }), true)} />
                    </td>
                    <td className="w-36 px-1">
                      <NumberCell
                        value={p.volatility}
                        placeholder={`${DEFAULT_VOLATILITY} default`}
                        onCommit={(v) => (v === undefined ? (update(i, { volatility: undefined }), true) : v >= 0 && v <= 5 && (update(i, { volatility: v || undefined }), true))}
                      />
                    </td>
                    <td className={cn('num px-3 text-right text-[13px]', value < 0 ? 'text-loss' : 'text-ink/90')}>{formatCurrency(value)}</td>
                    <td className="w-48 px-3">
                      <div className="flex items-center justify-end gap-2.5">
                        <div className="h-1.5 w-20 overflow-hidden rounded-full bg-ink/5">
                          <div className="h-full rounded-full" style={{ width: `${weight * 100}%`, background: PALETTE[i % PALETTE.length] }} />
                        </div>
                        <span className="num w-14 text-right text-[13px] text-ink/75">{formatPercent(weight, 1)}</span>
                      </div>
                    </td>
                    <td className="pr-3 text-right">
                      <button type="button" onClick={() => remove(i)} className="rounded p-1 text-ink/40 opacity-0 transition hover:bg-loss/10 hover:text-loss group-hover:opacity-100 focus:opacity-100" aria-label={`Remove ${p.symbol}`}>
                        <Trash2 className="h-3.5 w-3.5" />
                      </button>
                    </td>
                  </motion.tr>
                ))}
              </AnimatePresence>
            </tbody>
            <tfoot>
              <tr className="text-[12px]">
                <td className="px-3 py-3" colSpan={4}>
                  <button type="button" onClick={add} className="btn-ghost">
                    <Plus className="h-3.5 w-3.5" /> Add position
                  </button>
                </td>
                <td className="num px-3 text-right font-semibold text-ink">{formatCurrency(stats.netValue)}</td>
                <td className="num px-3 text-right text-ink/50">{positions.length ? '100.0%' : '—'}</td>
                <td />
              </tr>
            </tfoot>
          </table>
        </div>
        {!positions.length && <p className="p-8 text-center text-sm text-ink/50">No positions. Load the sample, upload a CSV, or add a row.</p>}
      </GlassCard>
    </div>
  );
}
