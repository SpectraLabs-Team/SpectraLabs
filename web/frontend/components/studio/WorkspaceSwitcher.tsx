'use client';

import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import { Check, ChevronDown, Layers, Plus, User, Users } from 'lucide-react';
import { Spinner } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';

type Role = 'owner' | 'editor' | 'viewer';
type Workspace = { id: string; name: string; slug: string; personal: boolean; role: Role };

const ROLE_LABEL: Record<Role, string> = { owner: 'Owner', editor: 'Editor', viewer: 'Viewer' };
const MAX_NAME = 60;

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

/**
 * Studio header control: shows the active workspace and the user's role in it; lists the user's
 * workspaces to switch between; creates a new team workspace. Switching reloads the page so every
 * panel refetches under the new workspace.
 */
export function WorkspaceSwitcher({ className }: { className?: string }) {
  const [workspaces, setWorkspaces] = useState<Workspace[] | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');
  const [busy, setBusy] = useState<string | null>(null); // workspace id being switched to, or 'new'
  const [error, setError] = useState('');
  const root = useRef<HTMLDivElement>(null);

  const load = useCallback(async () => {
    try {
      const data = await call<{ workspaces: Workspace[]; activeId: string }>('/api/workspaces');
      setWorkspaces(data.workspaces);
      setActiveId(data.activeId);
    } catch {
      setWorkspaces([]);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  // Close on outside click or Escape.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => root.current && !root.current.contains(e.target as Node) && setOpen(false);
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false);
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const active = workspaces?.find((w) => w.id === activeId);

  const switchTo = async (id: string) => {
    if (id === activeId) return setOpen(false);
    setBusy(id);
    setError('');
    try {
      await call('/api/workspaces/active', 'PUT', { workspaceId: id });
      window.location.reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not switch workspace.');
      setBusy(null);
    }
  };

  const create = async (e: FormEvent) => {
    e.preventDefault();
    const clean = name.replace(/\s+/g, ' ').trim();
    if (!clean) return;
    setBusy('new');
    setError('');
    try {
      await call('/api/workspaces', 'POST', { name: clean });
      window.location.reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not create the workspace.');
      setBusy(null);
    }
  };

  if (!workspaces || !active) {
    return <span className={cn('h-8 w-36 animate-pulse rounded-lg bg-ink/[0.05]', className)} aria-hidden />;
  }

  return (
    <div ref={root} className={cn('relative', className)}>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="listbox"
        aria-expanded={open}
        title={`Workspace: ${active.name} (${ROLE_LABEL[active.role]})`}
        className="flex max-w-[220px] items-center gap-2 rounded-lg border border-ink/10 bg-ink/[0.03] py-1.5 pl-2.5 pr-2 text-[12.5px] font-medium text-ink/85 transition hover:border-accent/30 hover:bg-accent/[0.05]"
      >
        {active.personal ? <User className="h-3.5 w-3.5 shrink-0 text-ink/45" /> : <Users className="h-3.5 w-3.5 shrink-0 text-accent" />}
        <span className="truncate">{active.name}</span>
        {active.role !== 'owner' && <span className="shrink-0 rounded bg-ink/[0.06] px-1.5 py-px text-[10px] font-semibold text-ink/55">{ROLE_LABEL[active.role]}</span>}
        <ChevronDown className={cn('h-3.5 w-3.5 shrink-0 text-ink/40 transition', open && 'rotate-180')} />
      </button>

      {open && (
        <div className="absolute left-0 top-full z-40 mt-1.5 w-72 overflow-hidden rounded-xl border border-ink/10 bg-panel shadow-xl">
          <p className="flex items-center gap-1.5 px-3 pb-1 pt-2.5 font-mono text-[10px] font-medium uppercase tracking-[0.16em] text-ink/40">
            <Layers className="h-3 w-3" /> Workspaces
          </p>
          <ul role="listbox" aria-label="Workspaces" className="max-h-72 overflow-y-auto p-1.5 pt-0">
            {workspaces.map((w) => (
              <li key={w.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={w.id === activeId}
                  disabled={busy !== null}
                  onClick={() => switchTo(w.id)}
                  className={cn(
                    'flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[13px] transition hover:bg-ink/[0.05] disabled:opacity-60',
                    w.id === activeId && 'bg-accent/[0.07]',
                  )}
                >
                  {w.personal ? <User className="h-4 w-4 shrink-0 text-ink/40" /> : <Users className="h-4 w-4 shrink-0 text-accent/80" />}
                  <span className="min-w-0 flex-1">
                    <span className="block truncate font-medium text-ink">{w.name}</span>
                    <span className="block text-[11px] text-ink/45">{w.personal ? 'Only you' : ROLE_LABEL[w.role]}</span>
                  </span>
                  {busy === w.id ? <Spinner className="h-3.5 w-3.5" /> : w.id === activeId && <Check className="h-4 w-4 shrink-0 text-accent" />}
                </button>
              </li>
            ))}
          </ul>

          <div className="border-t border-ink/[0.07] p-1.5">
            {creating ? (
              <form onSubmit={create} className="space-y-2 p-1.5">
                <input
                  autoFocus
                  className="input py-1.5 text-[13px]"
                  placeholder="Team workspace name"
                  value={name}
                  maxLength={MAX_NAME}
                  onChange={(e) => setName(e.target.value)}
                  aria-label="New workspace name"
                />
                <div className="flex justify-end gap-1.5">
                  <button type="button" className="btn-ghost" onClick={() => (setCreating(false), setName(''), setError(''))}>
                    Cancel
                  </button>
                  <button type="submit" disabled={!name.trim() || busy !== null} className="btn-primary px-3 py-1.5 text-xs">
                    {busy === 'new' ? <Spinner className="h-3.5 w-3.5" /> : <Plus className="h-3.5 w-3.5" />} Create
                  </button>
                </div>
              </form>
            ) : (
              <button
                type="button"
                onClick={() => setCreating(true)}
                className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-[13px] font-medium text-ink/70 transition hover:bg-ink/[0.05] hover:text-ink"
              >
                <Plus className="h-4 w-4" /> New team workspace
              </button>
            )}
            {error && <p className="px-2.5 pb-1.5 text-xs text-loss">{error}</p>}
          </div>
        </div>
      )}
    </div>
  );
}
