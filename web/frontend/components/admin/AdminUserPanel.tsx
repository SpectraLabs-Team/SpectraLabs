'use client';

import { useCallback, useEffect, useState } from 'react';
import { Ban, CheckCircle2, Shield, ShieldOff } from 'lucide-react';
import { useReauth } from '@/components/auth/useReauth';
import { Spinner } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';

type Detail = {
  user: {
    id: string;
    email: string;
    name: string;
    role: 'admin' | 'user';
    status: 'active' | 'disabled';
    createdAt: number;
    passwordChangedAt: number | null;
    twoFactor: boolean;
    authenticator: 'off' | 'pending' | 'on';
    recoveryCodesLeft: number;
    failedLogins: number;
    lockedUntil: number | null;
  };
  workspaces: { id: string; name: string; personal: boolean; role: string; members: number }[];
  sessions: { id: string; device: string; ip: string | null; lastSeenAt: number }[];
  events: { seq: string; occurredAt: number; action: string; outcome: string; actorEmail: string | null; ip: string | null }[];
};

const when = (ms: number) => new Date(ms).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });

/**
 * Admin → one account (M19 task 13): security, workspaces, devices and recent activity, plus
 * disable / re-enable and grant / remove admin. Changes ask for a fresh "confirm it's you" and are
 * audit-logged; opening this panel is logged too.
 */
export function AdminUserPanel({ userId, self, onChanged }: { userId: string; self: boolean; onChanged: () => void }) {
  const { call, dialog } = useReauth();
  const [d, setD] = useState<Detail | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState('');

  const load = useCallback(() => call<Detail>(`/api/admin/users/${userId}`).then(setD).catch((e) => setError(e.message)), [call, userId]);
  useEffect(() => {
    load();
  }, [load]);

  const change = async (key: string, body: Record<string, string>, question: string) => {
    if (!window.confirm(question)) return;
    setBusy(key);
    setError('');
    try {
      setD(await call<Detail>(`/api/admin/users/${userId}`, 'PATCH', body));
      onChanged();
    } catch (e) {
      if (!(e instanceof Error && e.message === 'Cancelled.')) setError(e instanceof Error ? e.message : 'Could not update the account.');
    } finally {
      setBusy(null);
    }
  };

  if (!d) return <div className="py-4">{error ? <p className="text-xs text-loss">{error}</p> : <Spinner className="h-4 w-4" />}</div>;
  const u = d.user;

  return (
    <div className="space-y-4 rounded-xl border border-ink/[0.08] bg-canvas/50 p-4 text-[12.5px]">
      {dialog}
      <div className="flex flex-wrap items-center gap-2">
        <span className={cn('rounded-full px-2 py-0.5 text-[11px] font-semibold', u.status === 'active' ? 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300' : 'bg-rose-500/15 text-rose-700 dark:text-rose-300')}>
          {u.status === 'active' ? 'Active' : 'Disabled'}
        </span>
        <span className="text-ink/55">
          {u.twoFactor ? (u.authenticator === 'on' ? `Authenticator app · ${u.recoveryCodesLeft} recovery codes left` : 'Email codes') : 'Password only'}
          {' · '}password {u.passwordChangedAt ? `changed ${when(u.passwordChangedAt)}` : 'never changed'}
          {u.lockedUntil ? ` · locked until ${when(u.lockedUntil)}` : u.failedLogins ? ` · ${u.failedLogins} recent failed logins` : ''}
        </span>
        {!self && (
          <div className="ml-auto flex flex-wrap gap-1.5">
            {u.status === 'active' ? (
              <button type="button" className="btn-ghost border border-ink/10 hover:text-loss" disabled={busy !== null} onClick={() => change('status', { status: 'disabled' }, `Disable ${u.email}? They are signed out everywhere and can't sign in until re-enabled.`)}>
                {busy === 'status' ? <Spinner className="h-3.5 w-3.5" /> : <Ban className="h-3.5 w-3.5" />} Disable
              </button>
            ) : (
              <button type="button" className="btn-ghost border border-ink/10" disabled={busy !== null} onClick={() => change('status', { status: 'active' }, `Re-enable ${u.email}?`)}>
                {busy === 'status' ? <Spinner className="h-3.5 w-3.5" /> : <CheckCircle2 className="h-3.5 w-3.5" />} Re-enable
              </button>
            )}
            {u.role === 'admin' ? (
              <button type="button" className="btn-ghost border border-ink/10" disabled={busy !== null} onClick={() => change('role', { role: 'user' }, `Remove admin from ${u.email}? They are signed out everywhere.`)}>
                {busy === 'role' ? <Spinner className="h-3.5 w-3.5" /> : <ShieldOff className="h-3.5 w-3.5" />} Remove admin
              </button>
            ) : (
              <button type="button" className="btn-ghost border border-ink/10" disabled={busy !== null} onClick={() => change('role', { role: 'admin' }, `Make ${u.email} a platform admin? They can see and manage every account. Two-step login is turned on for them.`)}>
                {busy === 'role' ? <Spinner className="h-3.5 w-3.5" /> : <Shield className="h-3.5 w-3.5" />} Make admin
              </button>
            )}
          </div>
        )}
        {self && <span className="ml-auto text-[11.5px] text-ink/45">This is you: another admin manages your account.</span>}
      </div>
      {error && <p className="text-xs text-loss">{error}</p>}

      <div className="grid gap-4 md:grid-cols-2">
        <div>
          <p className="mb-1.5 font-mono text-[10.5px] uppercase tracking-[0.12em] text-ink/45">Workspaces</p>
          <ul className="space-y-1">
            {d.workspaces.map((w) => (
              <li key={w.id} className="flex justify-between gap-3">
                <span className="truncate text-ink/80">{w.personal ? 'Personal' : w.name}</span>
                <span className="shrink-0 text-ink/50">
                  {w.role}
                  {w.personal ? '' : ` · ${w.members} member${w.members === 1 ? '' : 's'}`}
                </span>
              </li>
            ))}
          </ul>
        </div>
        <div>
          <p className="mb-1.5 font-mono text-[10.5px] uppercase tracking-[0.12em] text-ink/45">Signed in on</p>
          {d.sessions.length === 0 ? (
            <p className="text-ink/45">No active sessions.</p>
          ) : (
            <ul className="space-y-1">
              {d.sessions.map((s) => (
                <li key={s.id} className="flex justify-between gap-3">
                  <span className="truncate text-ink/80">{s.device}</span>
                  <span className="shrink-0 font-mono text-[11px] text-ink/50">
                    {s.ip ?? '—'} · {when(s.lastSeenAt)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>

      <div>
        <p className="mb-1.5 font-mono text-[10.5px] uppercase tracking-[0.12em] text-ink/45">Recent activity (by or about this account)</p>
        {d.events.length === 0 ? (
          <p className="text-ink/45">Nothing recorded yet.</p>
        ) : (
          <ul className="max-h-56 space-y-0.5 overflow-y-auto font-mono text-[11px]">
            {d.events.map((e) => (
              <li key={e.seq} className="grid grid-cols-[120px_1fr_auto] gap-2">
                <span className="text-ink/45">{when(e.occurredAt)}</span>
                <span className={cn('truncate', e.outcome === 'success' ? 'text-accent' : 'text-caution')}>
                  {e.action}
                  {e.outcome !== 'success' ? ` (${e.outcome})` : ''}
                  {e.actorEmail && e.actorEmail !== u.email ? ` · by ${e.actorEmail}` : ''}
                </span>
                <span className="text-ink/40">{e.ip ?? ''}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
