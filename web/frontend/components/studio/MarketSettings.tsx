'use client';

import { useEffect, useState } from 'react';
import Link from 'next/link';
import { CheckCircle2, CircleSlash, Database, KeyRound, PlugZap, RefreshCw, ScrollText, Server, ShieldCheck, Users, Wallet } from 'lucide-react';
import { API_BASE } from '@/lib/api';
import { DEFAULT_PAPER } from '@/lib/paperLedger';
import { cn, formatCurrency } from '@/lib/utils';
import { Badge, GlassCard, Spinner } from '@/components/ui/primitives';
import { WorkspaceActivity } from '@/components/audit/WorkspaceActivity';
import { BrokerKeys } from './BrokerKeys';
import { WorkspaceMembers } from './WorkspaceMembers';
import { usePortfolio } from './PortfolioContext';

type Probe = { status: 'idle' | 'checking' } | { status: 'done'; code: number; detail: string } | { status: 'error'; detail: string };

/** Probes GET /api/alpaca/account and reports exactly what the engine says. */
function useAlpacaProbe() {
  const [probe, setProbe] = useState<Probe>({ status: 'idle' });
  const run = async () => {
    setProbe({ status: 'checking' });
    try {
      const r = await fetch(`${API_BASE}/api/alpaca/account`);
      let detail = r.statusText;
      try {
        const body = await r.json();
        if (typeof body?.detail === 'string') detail = body.detail;
      } catch {
        /* keep status text */
      }
      setProbe({ status: 'done', code: r.status, detail });
    } catch {
      setProbe({ status: 'error', detail: `Cannot reach the engine at ${API_BASE}.` });
    }
  };
  useEffect(() => {
    run();
  }, []);
  return { probe, run };
}

export function MarketSettings() {
  const { paper, setPaper, account, engine } = usePortfolio();
  const { probe, run } = useAlpacaProbe();
  const [capital, setCapital] = useState(String(paper.startingCapital));
  const [limit, setLimit] = useState(String(paper.leverageLimit));
  useEffect(() => {
    setCapital(String(paper.startingCapital));
    setLimit(String(paper.leverageLimit));
  }, [paper]);

  const capitalN = Number(capital);
  const limitN = Number(limit);
  const valid = Number.isFinite(capitalN) && capitalN > 0 && Number.isFinite(limitN) && limitN > 0 && limitN <= 10;

  return (
    <div className="grid gap-5 xl:grid-cols-2">
      <Link
        href="/account"
        className="flex items-center justify-between gap-4 rounded-2xl border border-ink/[0.08] bg-ink/[0.02] px-5 py-3.5 text-sm transition hover:border-accent/30 hover:bg-accent/[0.04] xl:col-span-2"
      >
        <span className="flex items-center gap-2 text-ink/75">
          <ShieldCheck className="h-4 w-4 text-accent" /> Profile, password and two-factor login now live on your Account page.
        </span>
        <span className="shrink-0 font-semibold text-accent">Open Account →</span>
      </Link>
      <GlassCard as="section" className="p-5">
        <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
          <Wallet className="h-4 w-4 text-profit" /> Paper trading ledger
        </h2>
        <p className="mt-1 text-xs leading-5 text-ink/50">
          A simulated account kept in this browser. The current book is assumed bought at its CSV prices from the starting capital. With no market data feed it is not re-marked, so P&amp;L stays zero.
        </p>
        <div className="mt-5 grid gap-4 sm:grid-cols-2">
          <label className="block">
            <span className="mb-1.5 block text-xs font-medium text-ink/60">Starting capital (USD)</span>
            <input className="input num" inputMode="decimal" value={capital} onChange={(e) => setCapital(e.target.value)} />
          </label>
          <label className="block">
            <span className="mb-1.5 block text-xs font-medium text-ink/60">Gross leverage limit (×)</span>
            <input className="input num" inputMode="decimal" value={limit} onChange={(e) => setLimit(e.target.value)} />
          </label>
        </div>
        <div className="mt-4 flex flex-wrap gap-2">
          <button type="button" className="btn-primary px-3 py-1.5 text-xs" disabled={!valid} onClick={() => setPaper({ startingCapital: capitalN, leverageLimit: limitN })}>
            Save
          </button>
          <button type="button" className="btn-secondary px-3 py-1.5 text-xs" onClick={() => setPaper(DEFAULT_PAPER)}>
            Reset to defaults
          </button>
        </div>
        <dl className="mt-5 grid grid-cols-2 gap-3 border-t border-ink/[0.06] pt-4 text-sm sm:grid-cols-4">
          {[
            ['Equity', formatCurrency(account.equity)],
            ['Cash', formatCurrency(account.cash)],
            ['Buying power', formatCurrency(account.buyingPower)],
            ['Gross leverage', `${account.grossLeverage.toFixed(2)}×`],
          ].map(([k, v]) => (
            <div key={k}>
              <dt className="text-[10.5px] uppercase tracking-wider text-ink/45">{k}</dt>
              <dd className="num mt-1 text-ink">{v}</dd>
            </div>
          ))}
        </dl>
        <p className="mt-3 text-[11px] text-ink/40">The 1.0× default matches the backtester ledger&apos;s gross leverage limit.</p>
      </GlassCard>

      <GlassCard as="section" className="p-5">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
              <PlugZap className="h-4 w-4 text-accent" /> Alpaca brokerage
            </h2>
            <p className="mt-1 text-xs leading-5 text-ink/50">Live trading goes through the engine&apos;s <code className="font-mono">/api/alpaca/*</code> proxy. This is the engine&apos;s current answer:</p>
          </div>
          <button type="button" onClick={run} className="btn-ghost shrink-0 whitespace-nowrap border border-ink/10" disabled={probe.status === 'checking'}>
            {probe.status === 'checking' ? <Spinner className="h-3.5 w-3.5" /> : <RefreshCw className="h-3.5 w-3.5" />} Re-check
          </button>
        </div>
        <div className="mt-4 rounded-lg border border-ink/[0.08] bg-canvas/60 p-4 font-mono text-[12px]">
          <p className="text-ink/50">GET {API_BASE}/api/alpaca/account</p>
          {probe.status === 'done' && (
            <>
              <p className={cn('mt-2', probe.code === 200 ? 'text-profit' : 'text-caution')}>
                → {probe.code} {probe.code === 501 ? 'Not Implemented' : ''}
              </p>
              <p className="mt-1 whitespace-pre-wrap text-ink/70">{probe.detail}</p>
            </>
          )}
          {probe.status === 'error' && <p className="mt-2 text-loss">{probe.detail}</p>}
          {probe.status === 'checking' && <p className="mt-2 text-ink/50">checking…</p>}
        </div>
        <div className="mt-4 space-y-2.5 text-[12.5px]">
          <div className="flex items-center gap-2.5">
            {probe.status === 'done' && probe.code === 200 ? <CheckCircle2 className="h-4 w-4 text-profit" /> : <CircleSlash className="h-4 w-4 text-caution" />}
            <span className="text-ink/70">Live Trading stays disabled in the sidebar until this route returns 200.</span>
          </div>
        </div>
        <div className="mt-5 border-t border-ink/[0.06] pt-4">
          <h3 className="mb-2.5 flex items-center gap-2 text-[12.5px] font-semibold text-ink">
            <KeyRound className="h-3.5 w-3.5 text-ink/50" /> Broker API keys
          </h3>
          <BrokerKeys />
        </div>
      </GlassCard>

      <GlassCard as="section" className="p-5">
        <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
          <Database className="h-4 w-4 text-accent-ice" /> Market data
        </h2>
        <div className="mt-3 flex items-center gap-2">
          <Badge tone="caution">No provider connected</Badge>
        </div>
        <p className="mt-3 text-xs leading-5 text-ink/55">
          Prices and volatilities come from the portfolio CSV (<code className="font-mono">symbol,quantity,price[,volatility]</code>). The data crate ingests CSV bars into a content-addressed store, but no live feed or provider connector is built yet.
        </p>
      </GlassCard>

      <GlassCard as="section" className="p-5">
        <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
          <Server className="h-4 w-4 text-accent" /> Engine connection
        </h2>
        <dl className="mt-4 space-y-2.5 text-[12.5px]">
          {[
            ['API route', `${API_BASE}/* → Rust engine`],
            ['Status', engine.status === 'online' ? `online · ${engine.health.engine} v${engine.health.engine_version}` : engine.status],
            ['Round trip', engine.status === 'online' ? `${engine.latencyMs.toFixed(0)} ms` : '—'],
            ['Configured by', 'ENGINE_URL (server-side) · signed-in proxy'],
          ].map(([k, v]) => (
            <div key={k} className="flex justify-between gap-4">
              <dt className="text-ink/50">{k}</dt>
              <dd className="num truncate text-right text-ink/90">{v}</dd>
            </div>
          ))}
        </dl>
      </GlassCard>
      <GlassCard as="section" className="p-5 xl:col-span-2">
        <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
          <Users className="h-4 w-4 text-accent" /> Members
        </h2>
        <p className="mb-4 mt-1 text-xs leading-5 text-ink/50">Who can use this workspace. Viewers read; editors create and run; owners manage members, keys and settings.</p>
        <WorkspaceMembers />
      </GlassCard>
      <GlassCard as="section" className="p-5 xl:col-span-2">
        <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
          <ScrollText className="h-4 w-4 text-accent" /> Workspace activity
        </h2>
        <p className="mb-4 mt-1 text-xs leading-5 text-ink/50">Key changes, settings and membership in this workspace, from the append-only audit log.</p>
        <WorkspaceActivity />
      </GlassCard>
    </div>
  );
}
