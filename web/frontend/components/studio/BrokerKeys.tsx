'use client';

import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { KeyRound, Lock, Trash2 } from 'lucide-react';
import { useReauth } from '@/components/auth/useReauth';
import { Spinner } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';

type SecretInfo = { kind: string; label: string; hint: string; updatedAt: number; lastUsedAt: number | null; createdBy: string | null };
type Kind = { kind: string; label: string; fields: { name: string; label: string }[] };
type State = {
  workspace: { name: string; role: string };
  canManage: boolean;
  configured: boolean;
  kinds: Kind[];
  secrets: SecretInfo[];
};

async function call<T>(url: string, method = 'GET', body?: unknown): Promise<T> {
  const res = await fetch(url, {
    method,
    credentials: 'same-origin',
    cache: 'no-store',
    headers: body ? { 'Content-Type': 'application/json' } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok || !data.ok) throw new Error(data.error || 'Something went wrong. Try again.');
  return data as T;
}

const when = (ms: number) => new Date(ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });

/**
 * Broker API keys for the active workspace. Write-only: a key can be added, replaced or deleted
 * (owners), but never read back. The page only ever sees its last 4 characters.
 */
export function BrokerKeys() {
  // Adding, replacing or deleting a key needs a fresh "confirm it's you".
  const { call: guarded, dialog } = useReauth();
  const [state, setState] = useState<State | null>(null);
  const [loadError, setLoadError] = useState('');
  const [editing, setEditing] = useState<string | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState('');

  const load = useCallback(async () => {
    try {
      setState(await call<State>('/api/secrets'));
      setLoadError('');
    } catch (err) {
      setLoadError(err instanceof Error ? err.message : 'Could not load keys.');
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const close = () => {
    setEditing(null);
    setValues({}); // drop the typed key from memory as soon as we're done with it
    setError('');
  };

  const save = async (e: FormEvent, kind: string) => {
    e.preventDefault();
    setBusy(kind);
    setError('');
    try {
      await guarded(`/api/secrets/${kind}`, 'PUT', values);
      close();
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not save the key.');
    } finally {
      setBusy(null);
    }
  };

  const remove = async (kind: string) => {
    setBusy(kind);
    setError('');
    try {
      await guarded(`/api/secrets/${kind}`, 'DELETE');
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not delete the key.');
    } finally {
      setBusy(null);
    }
  };

  if (loadError) return <p className="text-xs text-loss">{loadError}</p>;
  if (!state) return <div className="h-16 animate-pulse rounded-lg bg-ink/[0.04]" aria-hidden />;

  return (
    <div className="space-y-3">
      {dialog}
      <p className="flex items-start gap-2 text-[12px] leading-5 text-ink/55">
        <Lock className="mt-0.5 h-3.5 w-3.5 shrink-0 text-ink/40" />
        <span>
          Keys for <span className="font-medium text-ink/80">{state.workspace.name}</span> are encrypted on the server and can&apos;t be read back, only replaced.
          {!state.canManage && ' Only workspace owners can change them.'}
          {state.canManage && ' Adding a key needs two-step login on, and asks you to confirm it’s you.'}
        </span>
      </p>
      {!state.configured && <p className="rounded-lg border border-caution/25 bg-caution/[0.06] px-3 py-2 text-xs text-amber-800 dark:text-amber-200/90">Key storage isn&apos;t configured on this server (SECRETS_MASTER_KEY).</p>}

      {state.kinds.map((k) => {
        const stored = state.secrets.find((s) => s.kind === k.kind);
        const open = editing === k.kind;
        return (
          <div key={k.kind} className="rounded-lg border border-ink/[0.08] bg-canvas/60 p-3.5">
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <p className="flex items-center gap-2 text-[13px] font-medium text-ink">
                  <KeyRound className="h-3.5 w-3.5 text-accent" /> {k.label}
                </p>
                <p className="mt-0.5 text-[11.5px] text-ink/50">
                  {stored ? (
                    <>
                      Key ID <span className="num text-ink/75">••••{stored.hint}</span> · updated {when(stored.updatedAt)}
                      {stored.createdBy ? ` by ${stored.createdBy}` : ''} · {stored.lastUsedAt ? `last used ${when(stored.lastUsedAt)}` : 'not used yet'}
                    </>
                  ) : (
                    'No key stored.'
                  )}
                </p>
              </div>
              {state.canManage && state.configured && !open && (
                <div className="flex shrink-0 gap-1.5">
                  <button type="button" className="btn-ghost border border-ink/10" onClick={() => (setEditing(k.kind), setValues({}), setError(''))}>
                    {stored ? 'Replace' : 'Add key'}
                  </button>
                  {stored && (
                    <button type="button" className="btn-ghost border border-ink/10 hover:text-loss" disabled={busy === k.kind} onClick={() => remove(k.kind)} aria-label={`Delete ${k.label} key`}>
                      {busy === k.kind ? <Spinner className="h-3.5 w-3.5" /> : <Trash2 className="h-3.5 w-3.5" />}
                    </button>
                  )}
                </div>
              )}
            </div>

            {open && (
              <form onSubmit={(e) => save(e, k.kind)} className="mt-3 space-y-2.5" autoComplete="off">
                {k.fields.map((f) => (
                  <label key={f.name} className="block">
                    <span className="mb-1 block text-[11.5px] font-medium text-ink/60">{f.label}</span>
                    <input
                      className={cn('input num py-1.5 text-[13px]')}
                      type="password"
                      autoComplete="off"
                      spellCheck={false}
                      value={values[f.name] ?? ''}
                      onChange={(e) => setValues((v) => ({ ...v, [f.name]: e.target.value }))}
                    />
                  </label>
                ))}
                {error && <p className="text-xs text-loss">{error}</p>}
                <div className="flex justify-end gap-1.5">
                  <button type="button" className="btn-ghost" onClick={close}>
                    Cancel
                  </button>
                  <button type="submit" className="btn-primary px-3 py-1.5 text-xs" disabled={busy !== null || k.fields.some((f) => !values[f.name]?.trim())}>
                    {busy === k.kind ? <Spinner className="h-3.5 w-3.5" /> : <Lock className="h-3.5 w-3.5" />} Save encrypted
                  </button>
                </div>
              </form>
            )}
          </div>
        );
      })}
      {error && !editing && <p className="text-xs text-loss">{error}</p>}
    </div>
  );
}
