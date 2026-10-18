"use client";

import { useEffect, useMemo, useState } from 'react';
import { getSamplePortfolio } from '@/lib/api';

type Position = { symbol: string; quantity: number; price: number };

export default function PortfolioPage() {
  const [csv, setCsv] = useState<string | null>(null);
  const [error, setError] = useState('');
  useEffect(() => { getSamplePortfolio().then(setCsv).catch((e: Error) => setError(e.message)); }, []);
  const rows = useMemo(() => {
    if (!csv) return [];
    const lines = csv.trim().split(/\r?\n/);
    const headers = lines.shift()?.split(',').map((h) => h.trim().toLowerCase()) ?? [];
    return lines.map((line) => line.split(',').map((v) => v.trim())).filter((values) => values.length >= headers.length)
      .map((values) => Object.fromEntries(headers.map((header, i) => [header, values[i]])))
      .map((item) => ({ symbol: String(item.symbol ?? ''), quantity: Number(item.quantity), price: Number(item.price) }))
      .filter((item) => item.symbol && Number.isFinite(item.quantity) && Number.isFinite(item.price));
  }, [csv]);
  const total = rows.reduce((sum, row) => sum + row.quantity * row.price, 0);

  return <div className="mx-auto max-w-7xl space-y-7 p-5 sm:p-8">
    <header><p className="mb-2 text-xs font-semibold uppercase tracking-[0.2em] text-accent-primary">Positions</p><h1 className="text-3xl font-semibold tracking-tight">Portfolio</h1><p className="mt-2 text-sm text-foreground-muted">Current sample portfolio loaded from the API.</p></header>
    {error && <div role="alert" className="rounded-xl border border-accent-warning/30 bg-accent-warning/10 p-4 text-sm text-foreground-muted">Portfolio data is unavailable: {error}</div>}
    <section className="grid gap-3 sm:grid-cols-3">
      {[['Market value', csv ? total.toLocaleString(undefined, { style: 'currency', currency: 'USD' }) : 'Loading…'], ['Positions', csv ? String(rows.length) : '—'], ['Source', 'Sample portfolio API']].map(([label, value]) => <article key={label} className="rounded-xl border border-border bg-surface p-5"><p className="text-sm text-foreground-muted">{label}</p><p className="mt-3 text-xl font-semibold">{value}</p></article>)}
    </section>
    <section className="overflow-hidden rounded-xl border border-border bg-surface">
      <div className="border-b border-border px-5 py-4"><h2 className="font-semibold">Positions</h2><p className="mt-1 text-xs text-foreground-subtle">Quantities and prices from the configured CSV input.</p></div>
      <div className="overflow-x-auto"><table className="w-full min-w-[500px] text-sm"><thead><tr className="text-left text-xs uppercase tracking-wide text-foreground-subtle"><th className="px-5 py-3 font-medium">Symbol</th><th className="px-5 py-3 text-right font-medium">Quantity</th><th className="px-5 py-3 text-right font-medium">Price</th><th className="px-5 py-3 text-right font-medium">Market value</th></tr></thead><tbody>{rows.map((row) => <tr key={row.symbol} className="border-t border-border-subtle"><td className="px-5 py-3 font-medium">{row.symbol}</td><td className="px-5 py-3 text-right tabular-nums">{row.quantity.toLocaleString()}</td><td className="px-5 py-3 text-right tabular-nums">{row.price.toLocaleString(undefined, { style: 'currency', currency: 'USD' })}</td><td className="px-5 py-3 text-right tabular-nums">{(row.quantity * row.price).toLocaleString(undefined, { style: 'currency', currency: 'USD' })}</td></tr>)}{csv && rows.length === 0 && <tr><td colSpan={4} className="px-5 py-10 text-center text-foreground-muted">No valid positions were found in the CSV.</td></tr>}</tbody></table></div>
    </section>
  </div>;
}
