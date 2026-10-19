'use client';

import { useEffect, useState, type FormEvent } from 'react';
import { Lock, ShieldCheck, ShieldOff } from 'lucide-react';
import { GlassCard, Spinner } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';

type State = { status: 'loading' } | { status: 'ready'; enabled: boolean; enforced: boolean; reason?: string | null; method?: 'totp' | 'email' } | { status: 'error'; message: string };

/** Studio → Settings: choose password-only (1FA) or password + emailed code (2FA) at login. */
export function AccountSecurity() {
  const [state, setState] = useState<State>({ status: 'loading' });
  const [confirming, setConfirming] = useState(false); // asking for the password to turn 2FA off
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');

  useEffect(() => {
    fetch('/api/auth/2fa', { cache: 'no-store', credentials: 'same-origin' })
      .then((r) => r.json())
      .then((d) => setState(d.ok ? { status: 'ready', enabled: d.enabled, enforced: d.enforced, reason: d.reason, method: d.method } : { status: 'error', message: d.error || 'Could not load.' }))
      .catch(() => setState({ status: 'error', message: 'Could not load your security settings.' }));
  }, []);

  const save = async (enabled: boolean, pw?: string) => {
    setBusy(true);
    setError('');
    setNotice('');
    try {
      const r = await fetch('/api/auth/2fa', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'same-origin',
        body: JSON.stringify({ enabled, password: pw }),
      });
      const d = await r.json();
      if (!r.ok || !d.ok) throw new Error(d.error || 'Could not update the setting.');
      setState({ status: 'ready', enabled: d.enabled, enforced: d.enforced, reason: d.reason, method: d.method });
      setConfirming(false);
      setPassword('');
      setNotice(d.enabled ? (d.method === 'totp' ? 'Two-factor login is on. Your next login will ask for a code from your authenticator app.' : 'Two-factor login is on. Your next login will ask for an emailed code.') : 'Two-factor login is off. You’ll log in with your password only.');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not update the setting.');
    } finally {
      setBusy(false);
    }
  };

  const submitOff = (e: FormEvent) => {
    e.preventDefault();
    if (password) save(false, password);
  };

  const on = state.status === 'ready' && state.enabled;

  return (
    <GlassCard as="section" className="p-5 xl:col-span-2">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="min-w-0">
          <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
            {on ? <ShieldCheck className="h-4 w-4 text-profit" /> : <ShieldOff className="h-4 w-4 text-caution" />} Login security
          </h2>
          <p className="mt-1 max-w-xl text-xs leading-5 text-ink/55">
            Choose how you log in. With two-factor login, a code from your email is needed on top of your password, so a leaked password alone can&apos;t open your account.
          </p>
        </div>

        {state.status === 'loading' && <Spinner className="h-4 w-4" />}
        {state.status === 'ready' && (
          <div role="radiogroup" aria-label="Login method" className="inline-flex rounded-xl border border-ink/10 bg-ink/[0.03] p-1 text-[12.5px] font-semibold">
            <button
              type="button"
              role="radio"
              aria-checked={!on}
              disabled={busy || state.enforced || !on}
              onClick={() => {
                setConfirming(true);
                setError('');
                setNotice('');
              }}
              title={state.enforced ? state.reason ?? 'Two-factor login is required for this account' : undefined}
              className={cn('rounded-lg px-3 py-1.5 transition disabled:cursor-not-allowed', !on ? 'bg-panel text-ink shadow-sm' : 'text-ink/55 hover:text-ink disabled:hover:text-ink/55')}
            >
              Password only
            </button>
            <button
              type="button"
              role="radio"
              aria-checked={on}
              disabled={busy || on}
              onClick={() => save(true)}
              className={cn('rounded-lg px-3 py-1.5 transition disabled:cursor-not-allowed', on ? 'bg-profit/15 text-profit shadow-sm' : 'text-ink/55 hover:text-ink')}
            >
              {busy && !confirming ? <Spinner className="h-3.5 w-3.5" /> : state.method === 'totp' ? 'Password + authenticator' : 'Password + email code'}
            </button>
          </div>
        )}
      </div>

      {state.status === 'ready' && state.enforced && <p className="mt-3 text-[12px] text-ink/50">{state.reason ?? 'Required for this account.'} It can&apos;t be turned off here.</p>}

      {confirming && state.status === 'ready' && (
        <form onSubmit={submitOff} className="mt-4 flex flex-wrap items-end gap-3 rounded-xl border border-caution/25 bg-caution/[0.05] p-3">
          <label className="min-w-[220px] flex-1">
            <span className="block text-[12px] font-medium text-ink/70">Confirm your password to turn two-factor login off</span>
            <span className="relative mt-1.5 block">
              <Lock className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-ink/40" />
              <input
                type="password"
                autoComplete="current-password"
                autoFocus
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className="w-full rounded-xl border border-ink/15 bg-canvas/70 py-2 pl-9 pr-3 text-sm text-ink outline-none focus:border-cyan-500 focus:ring-2 focus:ring-cyan-500/25"
              />
            </span>
          </label>
          <div className="flex gap-2">
            <button type="button" onClick={() => (setConfirming(false), setPassword(''), setError(''))} className="btn-ghost border border-ink/10">
              Cancel
            </button>
            <button type="submit" disabled={!password || busy} className="btn-primary">
              {busy ? <Spinner className="h-3.5 w-3.5" /> : null} Turn off
            </button>
          </div>
        </form>
      )}

      {state.status === 'error' && <p className="mt-3 text-[12px] text-loss">{state.message}</p>}
      {error && <p className="mt-3 text-[12px] text-loss">{error}</p>}
      {notice && <p className="mt-3 text-[12px] text-profit">{notice}</p>}
    </GlassCard>
  );
}
