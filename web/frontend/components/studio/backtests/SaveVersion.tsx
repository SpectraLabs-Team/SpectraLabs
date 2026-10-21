'use client';

import { useState } from 'react';
import { BookmarkPlus, X } from 'lucide-react';
import { ApiError, detailErrors } from '@/lib/api';
import { createStrategy, saveStrategyVersion, type ParamValues, type SavedStrategy, type StrategyVersion } from '@/lib/backtests';
import { ErrorNote, Spinner } from '@/components/ui/primitives';
import { Field } from './Fields';

const NEW = '';

/**
 * "Save as version" (M2a): stores the built-in and parameters in the form as an immutable version of
 * a saved strategy, new or existing. The same built-in and parameters again give back the version
 * already saved, so saving twice never makes a duplicate; `onSaved` says which happened.
 */
export function SaveVersion({
  builtin,
  params,
  saved,
  disabled,
  onSaved,
}: {
  builtin: string;
  params: ParamValues;
  saved: SavedStrategy[];
  disabled?: boolean;
  onSaved: (version: StrategyVersion, created: boolean) => void;
}) {
  const [open, setOpen] = useState(false);
  const [target, setTarget] = useState(NEW);
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  const save = async () => {
    setBusy(true);
    setError('');
    try {
      let version: StrategyVersion;
      let created = true;
      if (target === NEW) {
        const d = await createStrategy({ name: name.trim(), builtin, params });
        version = d.versions[0];
      } else {
        const r = await saveStrategyVersion(target, { builtin, params });
        version = r.version;
        created = r.created;
      }
      setOpen(false);
      setName('');
      setTarget(version.strategy_id);
      onSaved(version, created);
    } catch (e) {
      if (e instanceof ApiError && e.code === 'strategy_exists') setError('A saved strategy already has that name; pick it from the list to add a version.');
      else if (e instanceof ApiError && e.status === 403) setError('Saving a strategy needs the editor role in this workspace.');
      else if (e instanceof ApiError && e.status === 422) {
        const f = detailErrors(e);
        setError(Object.entries(f).map(([k, m]) => (k ? `${k}: ${m}` : m)).join(' · ') || e.message);
      } else setError(e instanceof Error ? e.message : 'Could not save the version.');
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return (
      <div>
        <button type="button" className="btn-ghost" disabled={disabled} onClick={() => { setOpen(true); setError(''); }}>
          <BookmarkPlus className="h-3.5 w-3.5" /> Save as version
        </button>
      </div>
    );
  }

  const canSave = !busy && (target !== NEW || name.trim().length > 0);
  return (
    <div className="space-y-2.5 rounded-xl border border-ink/[0.08] bg-ink/[0.015] p-3">
      <p className="text-[11px] text-ink/50">Saves this strategy and these parameters as a version you can run again later. A version never changes.</p>
      <div className="grid gap-2 sm:grid-cols-2">
        <Field label="saved strategy">
          <select className="input py-1.5 text-[13px]" aria-label="Saved strategy to add a version to" value={target} disabled={busy} onChange={(e) => setTarget(e.target.value)}>
            <option value={NEW}>New saved strategy…</option>
            {saved.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
                {s.latest_version ? ` (v${s.latest_version})` : ''}
              </option>
            ))}
          </select>
        </Field>
        {target === NEW && (
          <Field label="name">
            <input
              className="input py-1.5 text-[13px]"
              aria-label="Name of the new saved strategy"
              value={name}
              maxLength={200}
              placeholder="e.g. SPY trend"
              disabled={busy}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && canSave) save();
              }}
            />
          </Field>
        )}
      </div>
      {error && <ErrorNote>{error}</ErrorNote>}
      <div className="flex items-center gap-2">
        <button type="button" className="btn-secondary px-3 py-1.5 text-xs" disabled={!canSave} onClick={save}>
          {busy ? <Spinner className="h-3.5 w-3.5" /> : <BookmarkPlus className="h-3.5 w-3.5" />} Save version
        </button>
        <button type="button" className="btn-ghost" disabled={busy} onClick={() => setOpen(false)}>
          <X className="h-3.5 w-3.5" /> Cancel
        </button>
      </div>
    </div>
  );
}
