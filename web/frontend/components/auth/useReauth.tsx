'use client';

import { useCallback, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { Lock, ShieldCheck } from 'lucide-react';
import { Spinner } from '@/components/ui/primitives';

type Pending = { resolve: (ok: boolean) => void };

/**
 * "Confirm it's you" for sensitive actions. `call(url, method, body)` works like a JSON fetch, but
 * if the server answers 403 { reauth: true } it asks for the password (or an authenticator /
 * recovery code), confirms with POST /api/auth/reauth, and retries once. Render `dialog`.
 */
export function useReauth(): { call: <T = Record<string, unknown>>(url: string, method?: string, body?: unknown) => Promise<T>; dialog: ReactNode } {
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<'password' | 'code'>('password');
  const [value, setValue] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const pending = useRef<Pending | null>(null);

  const ask = () =>
    new Promise<boolean>((resolve) => {
      pending.current = { resolve };
      setValue('');
      setError('');
      setMode('password');
      setOpen(true);
    });

  const close = (ok: boolean) => {
    setOpen(false);
    setValue('');
    pending.current?.resolve(ok);
    pending.current = null;
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!value.trim() || busy) return;
    setBusy(true);
    setError('');
    try {
      const res = await fetch('/api/auth/reauth', {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(mode === 'password' ? { password: value } : { code: value.trim() }),
      });
      const data = await res.json().catch(() => ({}));
      if (!res.ok || !data.ok) throw new Error(data.error || 'Could not confirm.');
      close(true);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not confirm.');
    } finally {
      setBusy(false);
    }
  };

  const call = useCallback(async <T,>(url: string, method = 'GET', body?: unknown): Promise<T> => {
    const send = () =>
      fetch(url, {
        method,
        credentials: 'same-origin',
        cache: 'no-store',
        headers: body !== undefined ? { 'Content-Type': 'application/json' } : undefined,
        body: body !== undefined ? JSON.stringify(body) : undefined,
      });
    let res = await send();
    let data = await res.json().catch(() => ({}));
    if (res.status === 403 && data?.reauth) {
      if (!(await ask())) throw new Error('Cancelled.');
      res = await send();
      data = await res.json().catch(() => ({}));
    }
    if (!res.ok || !data.ok) throw new Error(data.error || 'Something went wrong. Try again.');
    return data as T;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const dialog = open ? (
    <div className="fixed inset-0 z-[80] flex items-center justify-center bg-black/50 p-4" role="dialog" aria-modal="true" aria-labelledby="reauth-title" onKeyDown={(e) => e.key === 'Escape' && close(false)}>
      <form onSubmit={submit} className="w-full max-w-sm rounded-2xl border border-ink/10 bg-panel p-5 shadow-2xl">
        <h2 id="reauth-title" className="flex items-center gap-2 text-sm font-semibold text-ink">
          <ShieldCheck className="h-4 w-4 text-accent" /> Confirm it&apos;s you
        </h2>
        <p className="mt-1 text-xs leading-5 text-ink/55">This is a sensitive change. Confirm once and you won&apos;t be asked again for 10 minutes.</p>
        <label className="mt-4 block">
          <span className="mb-1.5 block text-[12px] font-medium text-ink/65">{mode === 'password' ? 'Password' : 'Authenticator or recovery code'}</span>
          <span className="relative block">
            <Lock className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-ink/40" />
            <input
              autoFocus
              type={mode === 'password' ? 'password' : 'text'}
              inputMode={mode === 'code' ? 'text' : undefined}
              autoComplete={mode === 'password' ? 'current-password' : 'one-time-code'}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              className="input pl-9"
            />
          </span>
        </label>
        <button type="button" className="mt-2 text-[11.5px] font-medium text-accent hover:underline" onClick={() => (setMode(mode === 'password' ? 'code' : 'password'), setValue(''), setError(''))}>
          {mode === 'password' ? 'Use an authenticator code instead' : 'Use my password instead'}
        </button>
        {error && <p className="mt-2 text-xs text-loss">{error}</p>}
        <div className="mt-4 flex justify-end gap-2">
          <button type="button" className="btn-ghost border border-ink/10" onClick={() => close(false)}>
            Cancel
          </button>
          <button type="submit" className="btn-primary px-3 py-1.5 text-xs" disabled={!value.trim() || busy}>
            {busy ? <Spinner className="h-3.5 w-3.5" /> : null} Confirm
          </button>
        </div>
      </form>
    </div>
  ) : null;

  return { call, dialog };
}
