'use client';

import { useCallback, useEffect, useState } from 'react';
import { Download, RefreshCw, ShieldAlert, ShieldCheck } from 'lucide-react';
import { Spinner } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';

type Outcome = 'success' | 'failure' | 'denied';
type AuditEvent = {
  seq: string;
  occurredAt: number;
  actorEmail: string | null;
  workspaceName: string | null;
  action: string;
  outcome: Outcome;
  targetType: string | null;
  targetId: string | null;
  ip: string | null;
  userAgent: string | null;
  details: Record<string, unknown>;
};
type Chain = { ok: boolean; events: number; head: string | null; breaks: { seq: string; problem: string }[] };

const CATEGORIES: [string, string][] = [
  ['', 'All events'],
  ['auth.', 'Sign-in & account'],
  ['admin.', 'Admin roles'],
  ['workspace.', 'Workspaces'],
  ['membership.', 'Members'],
  ['secret.', 'Broker keys'],
  ['order.', 'Orders'],
  ['kill_switch.', 'Kill switch'],
  ['audit.', 'Audit exports'],
];

const OUTCOME_STYLE: Record<Outcome, string> = {
  success: 'bg-emerald-500/12 text-emerald-700 dark:text-emerald-300',
  failure: 'bg-amber-500/15 text-amber-700 dark:text-amber-300',
  denied: 'bg-rose-500/15 text-rose-700 dark:text-rose-300',
};

const time = (ms: number) => new Date(ms).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit' });

/** One-line summary of an event's details (never contains secrets: the server never stores any). */
function summary(e: AuditEvent): string {
  const d = e.details || {};
  const parts: string[] = [];
  if (typeof d.reason === 'string') parts.push(d.reason.replace(/_/g, ' '));
  if (typeof d.method === 'string') parts.push(d.method);
  if (typeof d.email === 'string') parts.push(d.email);
  if (typeof d.name === 'string') parts.push(`"${d.name}"`);
  if (typeof d.from === 'string' && typeof d.to === 'string') parts.push(`"${d.from}" → "${d.to}"`);
  if (typeof d.hint === 'string') parts.push(`key ••••${d.hint}`);
  if (typeof d.failures === 'number') parts.push(`${d.failures} failed`);
  if (typeof d.ended === 'number') parts.push(`${d.ended} session(s) ended`);
  if (typeof d.rows === 'number') parts.push(`${d.rows} rows`);
  if (typeof d.purpose === 'string') parts.push(d.purpose);
  if (!parts.length && e.targetType) parts.push(`${e.targetType}${e.targetId && e.targetType === 'secret' ? ` ${e.targetId}` : ''}`);
  return parts.join(' · ');
}

/**
 * Filterable audit log with CSV export. `endpoint` is /api/admin/audit or
 * /api/workspaces/<id>/audit; `admin` adds the actor filter, the workspace column and the
 * hash-chain check.
 */
export function AuditLog({ endpoint, admin = false }: { endpoint: string; admin?: boolean }) {
  const [category, setCategory] = useState('');
  const [outcome, setOutcome] = useState('');
  const [actor, setActor] = useState('');
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [events, setEvents] = useState<AuditEvent[] | null>(null);
  const [nextBefore, setNextBefore] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [chain, setChain] = useState<Chain | null>(null);

  const query = useCallback(
    (extra: Record<string, string> = {}) => {
      const sp = new URLSearchParams();
      if (category) sp.set('action', category);
      if (outcome) sp.set('outcome', outcome);
      if (admin && actor.trim()) sp.set('actor', actor.trim());
      if (from) sp.set('from', new Date(`${from}T00:00:00`).toISOString());
      if (to) sp.set('to', new Date(new Date(`${to}T00:00:00`).getTime() + 86_400_000).toISOString());
      for (const [k, v] of Object.entries(extra)) sp.set(k, v);
      return sp.toString();
    },
    [category, outcome, actor, from, to, admin],
  );

  const load = useCallback(
    async (before?: string) => {
      setLoading(true);
      setError('');
      try {
        const res = await fetch(`${endpoint}?${query(before ? { before } : {})}`, { cache: 'no-store', credentials: 'same-origin' });
        const data = await res.json().catch(() => ({}));
        if (!res.ok || !data.ok) throw new Error(data.error || 'Could not load the audit log.');
        setEvents((prev) => (before && prev ? [...prev, ...data.events] : data.events));
        setNextBefore(data.nextBefore);
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Could not load the audit log.');
      } finally {
        setLoading(false);
      }
    },
    [endpoint, query],
  );

  const checkChain = useCallback(async () => {
    try {
      const res = await fetch('/api/admin/audit/verify', { cache: 'no-store', credentials: 'same-origin' });
      const data = await res.json();
      if (data.ok) setChain(data.chain);
    } catch {
      /* shown as unknown */
    }
  }, []);

  useEffect(() => {
    const t = setTimeout(() => load(), actor ? 300 : 0); // debounce typing in the actor box
    return () => clearTimeout(t);
  }, [load, actor]);

  useEffect(() => {
    if (admin) checkChain();
  }, [admin, checkChain]);

  const select = 'rounded-lg border border-ink/10 bg-canvas/70 px-2.5 py-1.5 text-[12.5px] text-ink focus:border-accent/60 focus:outline-none';

  return (
    <div>
      <div className="flex flex-wrap items-center gap-2">
        <select className={select} value={category} onChange={(e) => setCategory(e.target.value)} aria-label="Event type">
          {CATEGORIES.map(([v, label]) => (
            <option key={v} value={v}>
              {label}
            </option>
          ))}
        </select>
        <select className={select} value={outcome} onChange={(e) => setOutcome(e.target.value)} aria-label="Outcome">
          <option value="">Any outcome</option>
          <option value="success">Success</option>
          <option value="failure">Failure</option>
          <option value="denied">Denied</option>
        </select>
        {admin && <input className={cn(select, 'w-44')} placeholder="Actor email contains…" value={actor} onChange={(e) => setActor(e.target.value)} aria-label="Actor email" />}
        <input type="date" className={select} value={from} onChange={(e) => setFrom(e.target.value)} aria-label="From date" />
        <span className="text-xs text-ink/40">to</span>
        <input type="date" className={select} value={to} onChange={(e) => setTo(e.target.value)} aria-label="To date" />
        <div className="ml-auto flex gap-1.5">
          <button type="button" className="btn-ghost border border-ink/10" onClick={() => load()} disabled={loading} aria-label="Refresh">
            {loading ? <Spinner className="h-3.5 w-3.5" /> : <RefreshCw className="h-3.5 w-3.5" />}
          </button>
          <a className="btn-ghost border border-ink/10" href={`${endpoint}?${query({ format: 'csv' })}`} download>
            <Download className="h-3.5 w-3.5" /> CSV
          </a>
        </div>
      </div>

      {admin && chain && (
        <p className={cn('mt-3 flex flex-wrap items-center gap-1.5 text-[11.5px]', chain.ok ? 'text-ink/50' : 'font-medium text-loss')}>
          {chain.ok ? <ShieldCheck className="h-3.5 w-3.5 text-profit" /> : <ShieldAlert className="h-3.5 w-3.5" />}
          {chain.ok
            ? `Hash chain intact · ${chain.events.toLocaleString()} events`
            : `Hash chain BROKEN at seq ${chain.breaks.map((b) => b.seq).join(', ')}: rows were altered or removed outside the app.`}
          {chain.head && <span className="font-mono text-ink/40" title={`Head hash: ${chain.head}. Keep a copy elsewhere to detect removal of the newest rows.`}>head {chain.head.slice(0, 12)}…</span>}
        </p>
      )}

      {error && <p className="mt-3 text-xs text-loss">{error}</p>}

      <div className="mt-3 overflow-x-auto">
        <table className="w-full min-w-[640px] text-left text-[12px]">
          <thead>
            <tr className="border-b border-ink/[0.08] text-[10.5px] uppercase tracking-wider text-ink/45">
              <th className="py-2 pr-3 font-medium">Time</th>
              <th className="py-2 pr-3 font-medium">Event</th>
              <th className="py-2 pr-3 font-medium">Who</th>
              {admin && <th className="py-2 pr-3 font-medium">Workspace</th>}
              <th className="py-2 pr-3 font-medium">Details</th>
              <th className="py-2 font-medium">IP</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-ink/[0.05]">
            {events?.map((e) => (
              <tr key={e.seq} className="align-top">
                <td className="whitespace-nowrap py-2 pr-3 font-mono text-[11px] text-ink/50">{time(e.occurredAt)}</td>
                <td className="py-2 pr-3">
                  <span className="font-mono text-[11.5px] text-accent">{e.action}</span>
                  {e.outcome !== 'success' && <span className={cn('ml-1.5 rounded px-1.5 py-px text-[10px] font-semibold', OUTCOME_STYLE[e.outcome])}>{e.outcome}</span>}
                </td>
                <td className="max-w-[200px] truncate py-2 pr-3 text-ink/80" title={e.actorEmail ?? ''}>
                  {e.actorEmail ?? <span className="text-ink/35">system</span>}
                </td>
                {admin && <td className="max-w-[140px] truncate py-2 pr-3 text-ink/60">{e.workspaceName ?? '—'}</td>}
                <td className="max-w-[280px] truncate py-2 pr-3 text-ink/65" title={JSON.stringify(e.details)}>
                  {summary(e) || '—'}
                </td>
                <td className="whitespace-nowrap py-2 font-mono text-[11px] text-ink/45" title={e.userAgent ?? ''}>
                  {e.ip ?? '—'}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {events && events.length === 0 && <p className="py-6 text-center text-xs text-ink/45">No events match these filters.</p>}
        {!events && !error && <div className="mt-2 h-24 animate-pulse rounded-lg bg-ink/[0.04]" aria-hidden />}
      </div>
      {nextBefore && (
        <button type="button" className="btn-ghost mt-3 border border-ink/10" onClick={() => load(nextBefore)} disabled={loading}>
          {loading ? <Spinner className="h-3.5 w-3.5" /> : null} Load older events
        </button>
      )}
    </div>
  );
}
