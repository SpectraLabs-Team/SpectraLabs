'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Coins,
  FileSpreadsheet,
  Layers,
  Plus,
  RefreshCw,
  Search,
  Sparkles,
  Trash2,
  TrendingUp,
  X,
} from 'lucide-react';
import {
  createInstrument,
  deleteInstrument,
  listInstruments,
  SAMPLE_CONTRACTS,
  type CreateInstrumentPayload,
  type InstrumentItem,
  type InstrumentType,
} from '@/lib/instruments';
import { Badge, ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import { useToast } from '@/components/ui/StudioToast';

function formatDate(ns?: number | null): string {
  if (!ns) return '—';
  try {
    return new Date(ns / 1e6).toISOString().slice(0, 10);
  } catch {
    return '—';
  }
}

export function InstrumentsManager() {
  const { success, error: toastError } = useToast();
  const [instruments, setInstruments] = useState<InstrumentItem[]>([]);
  const [baseCurrency, setBaseCurrency] = useState('USD');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [filterType, setFilterType] = useState<string>('all');
  const [search, setSearch] = useState('');
  const [modalOpen, setModalOpen] = useState(false);
  const [seeding, setSeeding] = useState(false);
  const [deletingSymbol, setDeletingSymbol] = useState<string | null>(null);

  // Form State
  const [formType, setFormType] = useState<InstrumentType>('future');
  const [symbol, setSymbol] = useState('');
  const [currency, setCurrency] = useState('USD');
  const [multiplier, setMultiplier] = useState('50');
  const [tickSize, setTickSize] = useState('0.25');
  const [underlying, setUnderlying] = useState('');
  const [expiry, setExpiry] = useState('');
  const [maturity, setMaturity] = useState('');
  const [strike, setStrike] = useState('');
  const [right, setRight] = useState<'call' | 'put'>('call');
  const [couponRate, setCouponRate] = useState('');
  const [couponFrequency, setCouponFrequency] = useState('2');
  const [dayCount, setDayCount] = useState('act_act_icma');
  const [submitting, setSubmitting] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const res = await listInstruments();
      setInstruments(res.items ?? []);
      setBaseCurrency(res.base_currency || 'USD');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not load instruments.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const handleTypeChange = (type: InstrumentType) => {
    setFormType(type);
    if (type === 'future') {
      setMultiplier('50');
      setTickSize('0.25');
      setUnderlying('SPX');
    } else if (type === 'option') {
      setMultiplier('100');
      setTickSize('0.01');
      setUnderlying('SPY');
      setStrike('550.00');
    } else if (type === 'bond') {
      setMultiplier('1');
      setTickSize('0.01');
      setCouponRate('4.25');
      setCouponFrequency('2');
      setDayCount('act_act_icma');
    } else if (type === 'swap') {
      setMultiplier('1');
      setTickSize('0.01');
      setCouponRate('3.85');
      setCouponFrequency('2');
      setUnderlying('USD-SOFR');
    } else {
      setMultiplier('1');
      setTickSize('0.01');
    }
  };

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!symbol.trim()) {
      toastError('Missing Symbol', 'Please enter a contract symbol.');
      return;
    }
    setSubmitting(true);
    try {
      const payload: CreateInstrumentPayload = {
        symbol: symbol.trim().toUpperCase(),
        type: formType,
        currency: currency.trim().toUpperCase() || 'USD',
        multiplier: multiplier || '1',
        tick_size: tickSize || '0.01',
      };

      if (formType === 'future') {
        if (underlying) payload.underlying = underlying.trim().toUpperCase();
        if (expiry) payload.expiry = expiry;
      } else if (formType === 'option') {
        if (underlying) payload.underlying = underlying.trim().toUpperCase();
        payload.right = right;
        if (strike) payload.strike = strike;
        if (expiry) payload.expiry = expiry;
      } else if (formType === 'bond') {
        if (maturity) payload.maturity = maturity;
        if (couponRate) payload.coupon_rate = (parseFloat(couponRate) / 100).toString();
        payload.coupon_frequency = parseInt(couponFrequency, 10) || 2;
        payload.day_count = dayCount;
      } else if (formType === 'swap') {
        if (maturity) payload.maturity = maturity;
        if (couponRate) payload.coupon_rate = (parseFloat(couponRate) / 100).toString();
        payload.coupon_frequency = parseInt(couponFrequency, 10) || 2;
        if (underlying) payload.underlying = underlying.trim();
      }

      await createInstrument(payload);
      success('Contract Created', `Instrument ${payload.symbol} has been registered.`);
      setModalOpen(false);
      setSymbol('');
      await load();
    } catch (err) {
      toastError('Could not save contract', err instanceof Error ? err.message : 'Unknown error');
    } finally {
      setSubmitting(false);
    }
  };

  const handleDelete = async (sym: string) => {
    if (!confirm(`Are you sure you want to delete instrument "${sym}"?`)) return;
    setDeletingSymbol(sym);
    try {
      await deleteInstrument(sym);
      success('Instrument Deleted', `Removed ${sym} from registered contracts.`);
      await load();
    } catch (err) {
      toastError('Delete Failed', err instanceof Error ? err.message : 'Could not delete instrument');
    } finally {
      setDeletingSymbol(null);
    }
  };

  const handleSeedSamples = async () => {
    setSeeding(true);
    let count = 0;
    let lastError = '';
    try {
      for (const contract of SAMPLE_CONTRACTS) {
        try {
          await createInstrument(contract);
          count++;
        } catch (e) {
          const msg = e instanceof Error ? e.message : String(e);
          if (!msg.toLowerCase().includes('already has') && !msg.toLowerCase().includes('already exists')) {
            lastError = msg;
          }
        }
      }
      if (count > 0) {
        success('Sample Contracts Added', `Registered ${count} sample multi-asset contracts.`);
      } else if (lastError) {
        toastError('Seeding Failed', lastError);
      } else {
        success('Contracts Ready', 'All sample multi-asset contracts are already registered.');
      }
      await load();
    } catch (err) {
      toastError('Seeding Failed', err instanceof Error ? err.message : 'Error adding samples');
    } finally {
      setSeeding(false);
    }
  };

  const filtered = useMemo(() => {
    return instruments.filter((inst) => {
      const matchesType = filterType === 'all' || inst.type === filterType;
      const matchesSearch =
        !search ||
        inst.symbol.toLowerCase().includes(search.toLowerCase()) ||
        (inst.underlying && inst.underlying.toLowerCase().includes(search.toLowerCase()));
      return matchesType && matchesSearch;
    });
  }, [instruments, filterType, search]);

  const counts = useMemo(() => {
    const c: Record<string, number> = { all: instruments.length, future: 0, option: 0, bond: 0, swap: 0, equity: 0 };
    for (const inst of instruments) {
      c[inst.type] = (c[inst.type] || 0) + 1;
    }
    return c;
  }, [instruments]);

  return (
    <div className="space-y-6">
      {/* Overview Metric Cards */}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-5">
        <GlassCard className="p-4">
          <div className="flex items-center justify-between text-ink/50">
            <span className="text-xs font-medium uppercase tracking-wider">Total</span>
            <Layers className="h-4 w-4" />
          </div>
          <div className="mt-2 text-2xl font-bold text-ink">{counts.all}</div>
          <div className="mt-1 text-[11px] text-ink/40">Base: {baseCurrency}</div>
        </GlassCard>

        <GlassCard className="p-4">
          <div className="flex items-center justify-between text-ink/50">
            <span className="text-xs font-medium uppercase tracking-wider">Futures</span>
            <TrendingUp className="h-4 w-4 text-emerald-400" />
          </div>
          <div className="mt-2 text-2xl font-bold text-ink">{counts.future}</div>
          <div className="mt-1 text-[11px] text-ink/40">Rollable contracts</div>
        </GlassCard>

        <GlassCard className="p-4">
          <div className="flex items-center justify-between text-ink/50">
            <span className="text-xs font-medium uppercase tracking-wider">Options</span>
            <Coins className="h-4 w-4 text-purple-400" />
          </div>
          <div className="mt-2 text-2xl font-bold text-ink">{counts.option}</div>
          <div className="mt-1 text-[11px] text-ink/40">Calls & Puts (BSM)</div>
        </GlassCard>

        <GlassCard className="p-4">
          <div className="flex items-center justify-between text-ink/50">
            <span className="text-xs font-medium uppercase tracking-wider">Bonds</span>
            <FileSpreadsheet className="h-4 w-4 text-amber-400" />
          </div>
          <div className="mt-2 text-2xl font-bold text-ink">{counts.bond}</div>
          <div className="mt-1 text-[11px] text-ink/40">Fixed-income & yields</div>
        </GlassCard>

        <GlassCard className="p-4">
          <div className="flex items-center justify-between text-ink/50">
            <span className="text-xs font-medium uppercase tracking-wider">Swaps</span>
            <Sparkles className="h-4 w-4 text-sky-400" />
          </div>
          <div className="mt-2 text-2xl font-bold text-ink">{counts.swap}</div>
          <div className="mt-1 text-[11px] text-ink/40">Interest rate swaps</div>
        </GlassCard>
      </div>

      {/* Control Bar */}
      <GlassCard className="p-4">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex flex-wrap items-center gap-2">
            {[
              { id: 'all', label: `All (${counts.all})` },
              { id: 'future', label: `Futures (${counts.future})` },
              { id: 'option', label: `Options (${counts.option})` },
              { id: 'bond', label: `Bonds (${counts.bond})` },
              { id: 'swap', label: `Swaps (${counts.swap})` },
              { id: 'equity', label: `Equities (${counts.equity})` },
            ].map((tab) => (
              <button
                key={tab.id}
                type="button"
                onClick={() => setFilterType(tab.id)}
                className={`rounded-lg px-3 py-1.5 text-xs font-medium transition ${
                  filterType === tab.id
                    ? 'bg-accent text-white shadow-sm'
                    : 'bg-panel/40 text-ink/70 hover:bg-panel/70 hover:text-ink'
                }`}
              >
                {tab.label}
              </button>
            ))}
          </div>

          <div className="flex items-center gap-2">
            <div className="relative flex-1 sm:w-56">
              <Search className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-ink/40" />
              <input
                type="text"
                placeholder="Search symbol / underlying..."
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                className="w-full rounded-lg border border-ink/10 bg-panel/30 py-1.5 pl-8 pr-3 text-xs text-ink placeholder:text-ink/30 focus:border-accent focus:outline-none"
              />
            </div>

            <button
              type="button"
              onClick={handleSeedSamples}
              disabled={seeding}
              className="inline-flex items-center gap-1.5 rounded-lg border border-ink/15 bg-panel/60 px-3 py-1.5 text-xs font-medium text-ink/80 transition hover:bg-panel"
              title="Add standard pre-configured futures, options, and bonds"
            >
              {seeding ? <Spinner className="h-3.5 w-3.5" /> : <Sparkles className="h-3.5 w-3.5 text-amber-400" />}
              Seed Samples
            </button>

            <button
              type="button"
              onClick={() => setModalOpen(true)}
              className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white shadow transition hover:opacity-95"
            >
              <Plus className="h-3.5 w-3.5" />
              Add Contract
            </button>

            <button
              type="button"
              onClick={load}
              disabled={loading}
              className="rounded-lg border border-ink/10 p-1.5 text-ink/60 transition hover:bg-panel hover:text-ink"
              title="Refresh list"
            >
              <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
            </button>
          </div>
        </div>
      </GlassCard>

      {/* Main Table */}
      {error && <ErrorNote>{error}</ErrorNote>}

      <GlassCard className="overflow-hidden">
        {loading && instruments.length === 0 ? (
          <div className="flex h-48 items-center justify-center">
            <Spinner className="h-6 w-6 text-accent" />
          </div>
        ) : filtered.length === 0 ? (
          <div className="flex flex-col items-center justify-center gap-2 p-12 text-center">
            <Layers className="h-8 w-8 text-ink/20" />
            <p className="text-sm font-medium text-ink/70">No contracts found</p>
            <p className="text-xs text-ink/40">
              {search || filterType !== 'all'
                ? 'Try adjusting your search or filters.'
                : 'Click "Add Contract" or "Seed Samples" to add multi-asset contracts.'}
            </p>
          </div>
        ) : (
          <div className="max-h-[520px] overflow-auto">
            <table className="w-full min-w-[760px] text-left text-xs">
              <thead className="sticky top-0 bg-panel/95 backdrop-blur">
                <tr className="border-b border-ink/10 text-[10.5px] uppercase tracking-wider text-ink/45">
                  <th className="py-3 pl-5 pr-3 font-semibold">Symbol</th>
                  <th className="px-3 py-3 font-semibold">Type</th>
                  <th className="px-3 py-3 font-semibold">Underlying</th>
                  <th className="px-3 py-3 font-semibold">Currency</th>
                  <th className="px-3 py-3 text-right font-semibold">Multiplier</th>
                  <th className="px-3 py-3 text-right font-semibold">Tick Size</th>
                  <th className="px-3 py-3 font-semibold">Specs</th>
                  <th className="px-3 py-3 font-semibold">Expiry / Maturity</th>
                  <th className="py-3 pl-3 pr-5 text-right font-semibold">Action</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-ink/[0.04]">
                {filtered.map((item) => (
                  <tr key={item.symbol} className="transition hover:bg-white/[0.02]">
                    <td className="py-2.5 pl-5 pr-3 font-mono font-medium text-ink">{item.symbol}</td>
                    <td className="px-3 py-2.5">
                      <Badge
                        tone={
                          item.type === 'future'
                            ? 'profit'
                            : item.type === 'option'
                            ? 'accent'
                            : item.type === 'bond'
                            ? 'caution'
                            : 'neutral'
                        }
                      >
                        {item.type}
                      </Badge>
                    </td>
                    <td className="px-3 py-2.5 text-ink/70">{item.underlying || '—'}</td>
                    <td className="px-3 py-2.5 text-ink/70">{item.currency}</td>
                    <td className="px-3 py-2.5 text-right font-mono text-ink/80">{item.multiplier}</td>
                    <td className="px-3 py-2.5 text-right font-mono text-ink/60">{item.tick_size}</td>
                    <td className="px-3 py-2.5 text-ink/70">
                      {item.type === 'option' && (
                        <span>
                          {item.right?.toUpperCase()} @ strike {item.strike}
                        </span>
                      )}
                      {item.type === 'bond' && (
                        <span>
                          {item.coupon_rate ? `${(parseFloat(item.coupon_rate) * 100).toFixed(2)}%` : '—'} ×{' '}
                          {item.coupon_frequency}/yr ({item.day_count ?? 'act/act'})
                        </span>
                      )}
                      {item.type === 'swap' && (
                        <span>
                          Fixed {item.coupon_rate ? `${(parseFloat(item.coupon_rate) * 100).toFixed(2)}%` : '—'} ×{' '}
                          {item.coupon_frequency}/yr
                        </span>
                      )}
                      {item.type === 'future' && <span>Continuous roll eligible</span>}
                      {item.type === 'equity' && <span>Cash equity</span>}
                    </td>
                    <td className="px-3 py-2.5 font-mono text-ink/60">
                      {formatDate(item.expiry || item.maturity)}
                    </td>
                    <td className="py-2.5 pl-3 pr-5 text-right">
                      <button
                        type="button"
                        onClick={() => handleDelete(item.symbol)}
                        disabled={deletingSymbol === item.symbol}
                        className="rounded p-1 text-ink/40 transition hover:bg-loss/10 hover:text-loss"
                        title="Delete contract"
                      >
                        {deletingSymbol === item.symbol ? (
                          <Spinner className="h-3.5 w-3.5" />
                        ) : (
                          <Trash2 className="h-3.5 w-3.5" />
                        )}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </GlassCard>

      {/* Add Contract Modal */}
      {modalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm">
          <GlassCard className="w-full max-w-lg overflow-hidden border border-ink/20 shadow-2xl">
            <div className="flex items-center justify-between border-b border-ink/10 px-6 py-4">
              <div>
                <h3 className="text-base font-semibold text-ink">Add Multi-Asset Contract</h3>
                <p className="text-xs text-ink/50">Register a new instrument for backtests and live trading.</p>
              </div>
              <button
                type="button"
                onClick={() => setModalOpen(false)}
                className="rounded-lg p-1 text-ink/50 hover:bg-panel hover:text-ink"
              >
                <X className="h-4 w-4" />
              </button>
            </div>

            <form onSubmit={handleCreate} className="space-y-4 p-6">
              {/* Asset Type Selector */}
              <div>
                <label className="text-xs font-medium text-ink/70">Instrument Type</label>
                <div className="mt-1.5 grid grid-cols-5 gap-1.5">
                  {(['future', 'option', 'bond', 'swap', 'equity'] as InstrumentType[]).map((t) => (
                    <button
                      key={t}
                      type="button"
                      onClick={() => handleTypeChange(t)}
                      className={`rounded-lg py-1.5 text-xs font-medium capitalize transition ${
                        formType === t ? 'bg-accent text-white' : 'bg-panel/40 text-ink/70 hover:bg-panel'
                      }`}
                    >
                      {t}
                    </button>
                  ))}
                </div>
              </div>

              {/* Core Symbol and Currency */}
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="text-xs font-medium text-ink/70">Contract Symbol</label>
                  <input
                    type="text"
                    required
                    placeholder={
                      formType === 'future'
                        ? 'e.g. ESZ6'
                        : formType === 'option'
                        ? 'e.g. AAPL261218C00200000'
                        : formType === 'bond'
                        ? 'e.g. US10Y'
                        : 'e.g. USD-IRS-5Y'
                    }
                    value={symbol}
                    onChange={(e) => setSymbol(e.target.value)}
                    className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                  />
                </div>
                <div>
                  <label className="text-xs font-medium text-ink/70">Currency</label>
                  <input
                    type="text"
                    required
                    value={currency}
                    onChange={(e) => setCurrency(e.target.value)}
                    className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                  />
                </div>
              </div>

              {/* Multiplier & Tick Size */}
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="text-xs font-medium text-ink/70">Multiplier (Contract size)</label>
                  <input
                    type="number"
                    step="any"
                    required
                    value={multiplier}
                    onChange={(e) => setMultiplier(e.target.value)}
                    className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                  />
                </div>
                <div>
                  <label className="text-xs font-medium text-ink/70">Tick Size</label>
                  <input
                    type="number"
                    step="any"
                    required
                    value={tickSize}
                    onChange={(e) => setTickSize(e.target.value)}
                    className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                  />
                </div>
              </div>

              {/* Conditional: Futures fields */}
              {formType === 'future' && (
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="text-xs font-medium text-ink/70">Underlying / Chain</label>
                    <input
                      type="text"
                      placeholder="e.g. SPX or CL"
                      value={underlying}
                      onChange={(e) => setUnderlying(e.target.value)}
                      className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                    />
                  </div>
                  <div>
                    <label className="text-xs font-medium text-ink/70">Expiry Date</label>
                    <input
                      type="date"
                      value={expiry}
                      onChange={(e) => setExpiry(e.target.value)}
                      className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                    />
                  </div>
                </div>
              )}

              {/* Conditional: Options fields */}
              {formType === 'option' && (
                <>
                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <label className="text-xs font-medium text-ink/70">Underlying Stock</label>
                      <input
                        type="text"
                        placeholder="e.g. AAPL or SPY"
                        value={underlying}
                        onChange={(e) => setUnderlying(e.target.value)}
                        className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                      />
                    </div>
                    <div>
                      <label className="text-xs font-medium text-ink/70">Option Right</label>
                      <div className="mt-1 grid grid-cols-2 gap-1">
                        <button
                          type="button"
                          onClick={() => setRight('call')}
                          className={`rounded-lg py-1.5 text-xs font-medium transition ${
                            right === 'call' ? 'bg-profit text-white' : 'bg-panel/40 text-ink/70'
                          }`}
                        >
                          CALL
                        </button>
                        <button
                          type="button"
                          onClick={() => setRight('put')}
                          className={`rounded-lg py-1.5 text-xs font-medium transition ${
                            right === 'put' ? 'bg-loss text-white' : 'bg-panel/40 text-ink/70'
                          }`}
                        >
                          PUT
                        </button>
                      </div>
                    </div>
                  </div>
                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <label className="text-xs font-medium text-ink/70">Strike Price</label>
                      <input
                        type="number"
                        step="any"
                        placeholder="e.g. 550.00"
                        value={strike}
                        onChange={(e) => setStrike(e.target.value)}
                        className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                      />
                    </div>
                    <div>
                      <label className="text-xs font-medium text-ink/70">Expiry Date</label>
                      <input
                        type="date"
                        value={expiry}
                        onChange={(e) => setExpiry(e.target.value)}
                        className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                      />
                    </div>
                  </div>
                </>
              )}

              {/* Conditional: Bonds fields */}
              {formType === 'bond' && (
                <>
                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <label className="text-xs font-medium text-ink/70">Annual Coupon Rate (%)</label>
                      <input
                        type="number"
                        step="0.01"
                        placeholder="e.g. 4.25"
                        value={couponRate}
                        onChange={(e) => setCouponRate(e.target.value)}
                        className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                      />
                    </div>
                    <div>
                      <label className="text-xs font-medium text-ink/70">Coupons per Year</label>
                      <select
                        value={couponFrequency}
                        onChange={(e) => setCouponFrequency(e.target.value)}
                        className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                      >
                        <option value="1">1 (Annual)</option>
                        <option value="2">2 (Semi-annual)</option>
                        <option value="4">4 (Quarterly)</option>
                        <option value="12">12 (Monthly)</option>
                      </select>
                    </div>
                  </div>
                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <label className="text-xs font-medium text-ink/70">Maturity Date</label>
                      <input
                        type="date"
                        value={maturity}
                        onChange={(e) => setMaturity(e.target.value)}
                        className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                      />
                    </div>
                    <div>
                      <label className="text-xs font-medium text-ink/70">Day Count Convention</label>
                      <select
                        value={dayCount}
                        onChange={(e) => setDayCount(e.target.value)}
                        className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                      >
                        <option value="act_act_icma">Actual/Actual (ICMA)</option>
                        <option value="30_360">30/360 US</option>
                        <option value="act_360">Actual/360</option>
                        <option value="act_365f">Actual/365 Fixed</option>
                      </select>
                    </div>
                  </div>
                </>
              )}

              {/* Conditional: Swaps fields */}
              {formType === 'swap' && (
                <>
                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <label className="text-xs font-medium text-ink/70">Fixed Rate (%)</label>
                      <input
                        type="number"
                        step="0.01"
                        placeholder="e.g. 3.85"
                        value={couponRate}
                        onChange={(e) => setCouponRate(e.target.value)}
                        className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                      />
                    </div>
                    <div>
                      <label className="text-xs font-medium text-ink/70">Payments per Year</label>
                      <select
                        value={couponFrequency}
                        onChange={(e) => setCouponFrequency(e.target.value)}
                        className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                      >
                        <option value="1">1 (Annual)</option>
                        <option value="2">2 (Semi-annual)</option>
                        <option value="4">4 (Quarterly)</option>
                      </select>
                    </div>
                  </div>
                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <label className="text-xs font-medium text-ink/70">Maturity Date</label>
                      <input
                        type="date"
                        value={maturity}
                        onChange={(e) => setMaturity(e.target.value)}
                        className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                      />
                    </div>
                    <div>
                      <label className="text-xs font-medium text-ink/70">Yield Curve Name</label>
                      <input
                        type="text"
                        placeholder="e.g. USD-SOFR"
                        value={underlying}
                        onChange={(e) => setUnderlying(e.target.value)}
                        className="mt-1 w-full rounded-lg border border-ink/15 bg-panel/40 px-3 py-1.5 text-xs text-ink focus:border-accent focus:outline-none"
                      />
                    </div>
                  </div>
                </>
              )}

              {/* Action Buttons */}
              <div className="flex items-center justify-end gap-2 pt-3">
                <button
                  type="button"
                  onClick={() => setModalOpen(false)}
                  className="rounded-lg px-4 py-2 text-xs font-medium text-ink/70 hover:bg-panel hover:text-ink"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  disabled={submitting}
                  className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-4 py-2 text-xs font-medium text-white shadow hover:opacity-95 disabled:opacity-50"
                >
                  {submitting ? <Spinner className="h-3.5 w-3.5" /> : <Plus className="h-3.5 w-3.5" />}
                  Save Contract
                </button>
              </div>
            </form>
          </GlassCard>
        </div>
      )}
    </div>
  );
}
