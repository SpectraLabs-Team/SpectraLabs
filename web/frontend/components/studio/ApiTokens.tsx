'use client';

import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { Check, Copy, KeyRound, Plus, Trash2 } from 'lucide-react';
import { useReauth } from '@/components/auth/useReauth';
import { Spinner } from '@/components/ui/primitives';
import { useAuth } from '@/lib/auth';
import { cn } from '@/lib/utils';

type Role = 'owner' | 'editor' | 'viewer';
type Token = {
  id: string;
  kind: 'personal' | 'workspace';
  name: string;
  scopes: ('read' | 'run')[];
  hint: string;
  owner: { id: string; email: string } | null;
  createdBy: string | null;
  createdAt: number;
  expiresAt: number;
  lastUsedAt: number | null;
  lastUsedIp: string | null;
};

const day = (ms: number) => new Date(ms).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
const ago = (ms: number) => {
  const m = Math.round((Date.now() - ms) / 60_000);
  if (m < 2) return 'just now';
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  return h < 48 ? `${h} h ago` : day(ms);
};

/**
 * Settings → API tokens for the active workspace (M19 task 17). Tokens let scripts and notebooks
 * call the engine (`Authorization: Bearer …`). Shown once; owners see and can revoke every token
 * that reaches the workspace, others only their own.
 */
export function ApiTokens() {
  const { user } = useAuth();
  const { call, dialog } = useReauth();
  const [ws, setWs] = useState<{ id: string; name: string } | null>(null);
  const [role, setRole] = useState<Role | null>(null);
  const [tokens, setTokens] = useState<Token[] | null>(null);
  const [name, setName] = useState('');
  const [kind, setKind] = useState<'personal' | 'workspace'>('personal');
  const [access, setAccess] = useState<'read' | 'run'>('read');
  const [days, setDays] = useState(90);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [fresh, setFresh] = useState<{ token: string; name: string } | null>(null);
  const [copied, setCopied] = useState(false);

  const load = useCallback(async () => {
    try {
      const s = await call<{ workspace: { id: string; name: string } | null }>('/api/auth/session');
      if (!s.workspace) return;
      setWs(s.workspace);
      const d = await call<{ role: Role; tokens: Token[] }>(`/api/workspaces/${s.workspace.id}/tokens`);
      setRole(d.role);
      setTokens(d.tokens);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not load tokens.');
      setTokens((t) => t ?? []);
    }
  }, [call]);

  useEffect(() => {
    load();
  }, [load]);

  const create = async (e: FormEvent) => {
    e.preventDefault();
    if (!ws || !name.trim()) return;
    setBusy('create');
    setError('');
    setFresh(null);
    setCopied(false);
    try {
      const d = await call<{ token: string; info: Token }>(`/api/workspaces/${ws.id}/tokens`, 'POST', {
        name: name.trim(),
        kind,
        scopes: access === 'run' ? ['read', 'run'] : ['read'],
        expiresInDays: days,
      });
      setFresh({ token: d.token, name: d.info.name });
      setName('');
      await load();
    } catch (err) {
      if (!(err instanceof Error && err.message === 'Cancelled.')) setError(err instanceof Error ? err.message : 'Could not create the token.');
    } finally {
      setBusy(null);
    }
  };

  const revoke = async (t: Token) => {
    if (!ws || !window.confirm(`Revoke "${t.name}"? Anything using it stops working immediately.`)) return;
    setBusy(t.id);
    setError('');
    try {
      await call(`/api/workspaces/${ws.id}/tokens/${t.id}`, 'DELETE');
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not revoke the token.');
    } finally {
      setBusy(null);
    }
  };

  if (tokens === null) return <div className="h-16 animate-pulse rounded-lg bg-ink/[0.04]" aria-hidden />;
  const canRun = role === 'owner' || role === 'editor';

  return (
    <div className="space-y-4">
      {dialog}

      {fresh && (
        <div className="rounded-xl border border-profit/30 bg-profit/[0.06] p-3.5">
          <p className="text-[12.5px] font-medium text-ink">Copy “{fresh.name}” now. You won’t be able to see it again.</p>
          <div className="mt-2 flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate rounded bg-canvas/80 px-2 py-1.5 font-mono text-[11.5px] text-ink">{fresh.token}</code>
            <button
              type="button"
              className="btn-ghost border border-ink/10"
              onClick={() => {
                navigator.clipboard?.writeText(fresh.token);
                setCopied(true);
              }}
            >
              {copied ? <Check className="h-3.5 w-3.5 text-profit" /> : <Copy className="h-3.5 w-3.5" />} {copied ? 'Copied' : 'Copy'}
            </button>
          </div>
          <p className="mt-2 font-mono text-[11px] text-ink/50">curl -H &quot;Authorization: Bearer $SPECTRA_TOKEN&quot; {typeof window !== 'undefined' ? window.location.origin : ''}/api/engine/api/sample-portfolio</p>
        </div>
      )}

      {tokens.length === 0 ? (
        <p className="text-[12.5px] text-ink/55">No API tokens yet. Make one to call the engine from a script or notebook.</p>
      ) : (
        <ul className="divide-y divide-ink/[0.06] rounded-xl border border-ink/[0.08]">
          {tokens.map((t) => {
            const mine = t.owner?.email === user?.email;
            return (
              <li key={t.id} className="flex flex-wrap items-center gap-x-3 gap-y-1.5 px-3.5 py-2.5">
                <KeyRound className="h-3.5 w-3.5 shrink-0 text-ink/40" />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[13px] font-medium text-ink">
                    {t.name} <span className="font-mono text-[11px] font-normal text-ink/45">…{t.hint}</span>
                  </p>
                  <p className="truncate text-[11.5px] text-ink/50">
                    {t.kind === 'workspace' ? 'Workspace token' : mine ? 'Your token' : `${t.owner?.email}’s token`} · expires {day(t.expiresAt)} ·{' '}
                    {t.lastUsedAt ? `used ${ago(t.lastUsedAt)}${t.lastUsedIp ? ` from ${t.lastUsedIp}` : ''}` : 'never used'}
                  </p>
                </div>
                <span className={cn('rounded-full px-2 py-0.5 text-[11px] font-semibold', t.scopes.includes('run') ? 'bg-accent/12 text-accent' : 'bg-ink/[0.06] text-ink/65')}>
                  {t.scopes.includes('run') ? 'Read + run' : 'Read only'}
                </span>
                <button type="button" className="btn-ghost border border-ink/10 hover:text-loss" aria-label={`Revoke ${t.name}`} disabled={busy !== null} onClick={() => revoke(t)}>
                  {busy === t.id ? <Spinner className="h-3.5 w-3.5" /> : <Trash2 className="h-3.5 w-3.5" />}
                </button>
              </li>
            );
          })}
        </ul>
      )}

      <form onSubmit={create} className="flex flex-wrap items-end gap-2">
        <label className="min-w-[180px] flex-1">
          <span className="mb-1 block text-[11.5px] font-medium text-ink/60">New token</span>
          <input className="input py-1.5 text-[13px]" placeholder="e.g. research notebook" maxLength={60} value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" />
        </label>
        {role === 'owner' && (
          <select aria-label="Token type" className="rounded-lg border border-ink/10 bg-canvas/70 px-2 py-2 text-[12.5px] text-ink" value={kind} onChange={(e) => setKind(e.target.value as 'personal' | 'workspace')}>
            <option value="personal">Personal · acts as you</option>
            <option value="workspace">Workspace · for shared jobs</option>
          </select>
        )}
        <select aria-label="Access" className="rounded-lg border border-ink/10 bg-canvas/70 px-2 py-2 text-[12.5px] text-ink" value={access} onChange={(e) => setAccess(e.target.value as 'read' | 'run')}>
          <option value="read">Read only</option>
          <option value="run" disabled={!canRun}>
            Read + run computations
          </option>
        </select>
        <select aria-label="Expires" className="rounded-lg border border-ink/10 bg-canvas/70 px-2 py-2 text-[12.5px] text-ink" value={days} onChange={(e) => setDays(Number(e.target.value))}>
          {[7, 30, 90, 365].map((d) => (
            <option key={d} value={d}>
              Expires in {d === 365 ? '1 year' : `${d} days`}
            </option>
          ))}
        </select>
        <button type="submit" className="btn-primary px-3 py-2 text-xs" disabled={!name.trim() || busy !== null}>
          {busy === 'create' ? <Spinner className="h-3.5 w-3.5" /> : <Plus className="h-3.5 w-3.5" />} Create
        </button>
      </form>

      {error && <p className="text-xs text-loss">{error}</p>}
    </div>
  );
}
