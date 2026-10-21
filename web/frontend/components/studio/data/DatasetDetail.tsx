'use client';

import { useCallback, useEffect, useState } from 'react';
import { Check, FileText, GitFork, History, LineChart, Pencil, Plus, Trash2, X } from 'lucide-react';
import { ApiError, detailErrors } from '@/lib/api';
import {
  createCorporateActionSet,
  deleteVersion,
  deriveDatasetVersion,
  getDataset,
  getVersion,
  getVersionRuns,
  listCorporateActionSets,
  updateDataset,
  type CorporateActionSetSummary,
  type DatasetDetail as Detail,
  type VersionDetail,
  type VersionRun,
} from '@/lib/datasets';
import { Badge, ErrorNote, GlassCard, Spinner } from '@/components/ui/primitives';
import { cn } from '@/lib/utils';
import { signedPct, tone } from '../backtests/format';
import { formatOffset } from '@/lib/datasets';
import { ago, bytes, int, range } from './format';
import { PriceChart } from './PriceChart';
import { QualityReport } from './QualityReport';

function Section({ title, icon: Icon, children, aside }: { title: string; icon: typeof History; children: React.ReactNode; aside?: React.ReactNode }) {
  return (
    <GlassCard as="section" className="p-5">
      <div className="mb-3 flex items-center justify-between gap-2">
        <h3 className="flex items-center gap-2 text-[13px] font-semibold text-ink">
          <Icon className="h-4 w-4 text-accent" /> {title}
        </h3>
        {aside}
      </div>
      {children}
    </GlassCard>
  );
}

/**
 * One dataset (M1 task 7): its name and description, version history, and for the chosen version
 * its quality report, a price chart per symbol, where it came from, and the runs that used it.
 */
export function DatasetDetail({ datasetId, canEdit, onBacktest, onChanged, onDeleted }: { datasetId: string; canEdit: boolean; onBacktest: (versionId: string) => void; onChanged: () => void; onDeleted: () => void }) {
  const [detail, setDetail] = useState<Detail | null>(null);
  const [versionId, setVersionId] = useState<string | null>(null);
  const [version, setVersion] = useState<VersionDetail | null>(null);
  const [runs, setRuns] = useState<VersionRun[] | null>(null);
  const [error, setError] = useState('');
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [editErrors, setEditErrors] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);

  // Corporate action derivation
  const [deriving, setDeriving] = useState(false);
  const [actionSets, setActionSets] = useState<CorporateActionSetSummary[]>([]);
  const [selectedSetId, setSelectedSetId] = useState<string>('');
  const [adjustSplits, setAdjustSplits] = useState(true);
  const [adjustDividends, setAdjustDividends] = useState(true);
  const [targetDatasetName, setTargetDatasetName] = useState('');
  const [derivingBusy, setDerivingBusy] = useState(false);
  const [deriveError, setDeriveError] = useState('');

  // Quick-create action set in modal
  const [creatingSet, setCreatingSet] = useState(false);
  const [newSetName, setNewSetName] = useState('');
  const [newSetDesc, setNewSetDesc] = useState('');
  const [newActionSymbol, setNewActionSymbol] = useState('');
  const [newActionKind, setNewActionKind] = useState<'split' | 'dividend'>('split');
  const [newActionExDate, setNewActionExDate] = useState('');
  const [newActionSplitFrom, setNewActionSplitFrom] = useState(1);
  const [newActionSplitTo, setNewActionSplitTo] = useState(2);
  const [newActionDividend, setNewActionDividend] = useState('');

  const load = useCallback(async () => {
    try {
      const d = await getDataset(datasetId);
      setDetail(d);
      setVersionId((v) => (v && d.versions.some((x) => x.id === v) ? v : d.versions[0]?.id ?? null));
      setError('');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not load the dataset.');
    }
  }, [datasetId]);

  useEffect(() => {
    setDetail(null);
    setVersionId(null);
    load();
  }, [load]);

  useEffect(() => {
    if (!versionId) return;
    let live = true;
    setVersion(null);
    setRuns(null);
    Promise.all([getVersion(versionId), getVersionRuns(versionId)])
      .then(([v, r]) => {
        if (!live) return;
        setVersion(v);
        setRuns(r);
      })
      .catch((e) => live && setError(e instanceof Error ? e.message : 'Could not load the version.'));
    return () => {
      live = false;
    };
  }, [versionId]);

  const save = async () => {
    if (!detail) return;
    setSaving(true);
    setEditErrors({});
    try {
      const body: { name?: string; description?: string } = {};
      if (name.trim() !== detail.name) body.name = name;
      if (description !== detail.description) body.description = description;
      if (Object.keys(body).length) setDetail(await updateDataset(detail.id, body));
      setEditing(false);
      onChanged();
    } catch (e) {
      const f = detailErrors(e);
      setEditErrors(Object.keys(f).length ? f : { '': e instanceof ApiError && e.status === 409 ? 'Another dataset in this workspace has that name.' : e instanceof Error ? e.message : 'Could not save.' });
    } finally {
      setSaving(false);
    }
  };

  const remove = async (id: string, label: string) => {
    if (!window.confirm(`Delete ${label}? This can't be undone.`)) return;
    setDeleting(id);
    setError('');
    try {
      const r = await deleteVersion(id);
      onChanged();
      if (r.dataset_deleted) onDeleted();
      else {
        if (versionId === id) setVersionId(null);
        await load();
      }
    } catch (e) {
      setError(e instanceof ApiError && e.code === 'in_use' ? `${label} is used by a backtest, so it is kept (runs must stay reproducible).` : e instanceof Error ? e.message : 'Could not delete the version.');
    } finally {
      setDeleting(null);
    }
  };

  const openDeriveModal = async () => {
    setDeriveError('');
    setDeriving(true);
    setCreatingSet(false);
    if (version?.quality_report?.symbols?.[0]) {
      setNewActionSymbol(version.quality_report.symbols[0]);
    }
    try {
      const sets = await listCorporateActionSets();
      setActionSets(sets);
      if (sets.length > 0) {
        setSelectedSetId(sets[0].id);
      } else {
        setCreatingSet(true);
      }
    } catch (e) {
      setDeriveError(e instanceof Error ? e.message : 'Could not list corporate action sets.');
    }
  };

  const handleDerive = async () => {
    if (!versionId) return;
    setDerivingBusy(true);
    setDeriveError('');
    try {
      let setId = selectedSetId;
      if (creatingSet) {
        if (!newSetName.trim()) throw new Error('Please enter a name for the corporate action set.');
        if (!newActionSymbol.trim()) throw new Error('Please enter a symbol.');
        if (!newActionExDate.trim()) throw new Error('Please enter an ex-date.');
        const created = await createCorporateActionSet({
          name: newSetName.trim(),
          description: newSetDesc.trim(),
          actions: [
            {
              symbol: newActionSymbol.trim().toUpperCase(),
              action_kind: newActionKind,
              ex_date: newActionExDate.includes('T') ? newActionExDate : `${newActionExDate}T00:00:00Z`,
              split_from: newActionKind === 'split' ? Number(newActionSplitFrom) : undefined,
              split_to: newActionKind === 'split' ? Number(newActionSplitTo) : undefined,
              dividend_amount: newActionKind === 'dividend' ? newActionDividend.trim() : undefined,
            },
          ],
        });
        setId = created.id;
      }
      if (!setId) throw new Error('Please select or create a corporate action set.');

      const derived = await deriveDatasetVersion(versionId, {
        corporate_action_set_id: setId,
        adjust_splits: adjustSplits,
        adjust_dividends: adjustDividends,
        target_dataset_name: targetDatasetName.trim() || undefined,
      });

      setDeriving(false);
      onChanged();
      if (derived.dataset_id === datasetId) {
        setVersionId(derived.id);
        await load();
      } else {
        await load();
      }
    } catch (e) {
      setDeriveError(e instanceof Error ? e.message : 'Derivation failed.');
    } finally {
      setDerivingBusy(false);
    }
  };

  if (error && !detail) return <ErrorNote>{error}</ErrorNote>;
  if (!detail) {
    return (
      <GlassCard className="flex min-h-[200px] items-center justify-center">
        <Spinner />
      </GlassCard>
    );
  }
  const report = version?.quality_report;
  const settings = version?.source?.settings;
  return (
    <div className="space-y-5">
      <GlassCard as="section" className="p-5">
        {editing ? (
          <div className="space-y-2">
            <input className="input py-1.5 text-[15px] font-semibold" value={name} onChange={(e) => setName(e.target.value)} aria-label="Dataset name" maxLength={200} />
            {editErrors.name && <p className="text-[11px] text-loss">{editErrors.name}</p>}
            <textarea className="input min-h-[60px] py-1.5 text-[12.5px]" value={description} onChange={(e) => setDescription(e.target.value)} placeholder="What is this data? Source, adjustments, caveats…" aria-label="Description" maxLength={2000} />
            {(editErrors.description || editErrors['']) && <p className="text-[11px] text-loss">{editErrors.description ?? editErrors['']}</p>}
            <div className="flex gap-2">
              <button type="button" className="btn-primary px-3 py-1.5 text-xs" onClick={save} disabled={saving}>
                {saving ? <Spinner className="h-3.5 w-3.5" /> : <Check className="h-3.5 w-3.5" />} Save
              </button>
              <button type="button" className="btn-ghost text-xs" onClick={() => setEditing(false)}>
                <X className="h-3.5 w-3.5" /> Cancel
              </button>
            </div>
          </div>
        ) : (
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <h2 className="flex items-center gap-2 text-base font-semibold text-ink">
                {detail.name}
                {canEdit && (
                  <button
                    type="button"
                    className="btn-ghost p-1"
                    aria-label="Rename or describe"
                    onClick={() => {
                      setName(detail.name);
                      setDescription(detail.description);
                      setEditErrors({});
                      setEditing(true);
                    }}
                  >
                    <Pencil className="h-3.5 w-3.5" />
                  </button>
                )}
              </h2>
              <p className="mt-0.5 text-[12px] text-ink/55">{detail.description || <span className="italic text-ink/35">No description.</span>}</p>
            </div>
            <div className="flex items-center gap-2">
              {versionId && canEdit && (
                <button type="button" className="btn-secondary" onClick={openDeriveModal}>
                  <GitFork className="h-3.5 w-3.5" /> Derive adjusted
                </button>
              )}
              {versionId && (
                <button type="button" className="btn-primary" onClick={() => onBacktest(versionId)}>
                  <LineChart className="h-3.5 w-3.5" /> Backtest this dataset
                </button>
              )}
            </div>
          </div>
        )}
        {error && (
          <div className="mt-3">
            <ErrorNote>{error}</ErrorNote>
          </div>
        )}

        <div className="mt-4">
          <h3 className="mb-2 flex items-center gap-2 text-[11.5px] font-semibold uppercase tracking-wider text-ink/50">
            <History className="h-3.5 w-3.5" /> Versions
          </h3>
          <ul className="divide-y divide-ink/[0.05] rounded-lg border border-ink/[0.07]" aria-label="Versions">
            {detail.versions.map((v) => (
              <li key={v.id} className={cn('flex flex-wrap items-center gap-x-4 gap-y-1 px-3 py-2 text-[12px]', v.id === versionId && 'bg-accent/[0.07]')}>
                <button type="button" onClick={() => setVersionId(v.id)} className="flex min-w-0 flex-1 flex-wrap items-center gap-x-4 gap-y-1 text-left" aria-current={v.id === versionId || undefined}>
                  <span className="num font-semibold text-ink">v{v.version}</span>
                  <span className="num text-ink/65">{int(v.row_count)} bars</span>
                  <span className="num text-ink/50">{range(v.first_ts, v.last_ts)}</span>
                  <span className="truncate text-ink/50" title={v.filename ?? 'command line'}>
                    {v.filename ?? 'command line'}
                  </span>
                  {v.derivation_kind && <Badge tone="neutral">derived ({v.derivation_kind})</Badge>}
                  {v.rows_rejected > 0 && <Badge tone="loss">{int(v.rows_rejected)} rejected</Badge>}
                  {v.warning_count > 0 && <Badge tone="caution">{int(v.warning_count)} warnings</Badge>}
                  {v.run_count > 0 && <Badge tone="accent">{int(v.run_count)} runs</Badge>}
                  <span className="ml-auto text-ink/40">{ago(v.created_at)}</span>
                </button>
                {canEdit && (
                  <button
                    type="button"
                    className="btn-ghost p-1 text-ink/40 hover:text-loss disabled:opacity-30"
                    onClick={() => remove(v.id, `version ${v.version}`)}
                    disabled={v.run_count > 0 || deleting !== null}
                    title={v.run_count > 0 ? 'Used by a backtest: kept so runs stay reproducible' : `Delete version ${v.version}`}
                    aria-label={`Delete version ${v.version}`}
                  >
                    {deleting === v.id ? <Spinner className="h-3.5 w-3.5" /> : <Trash2 className="h-3.5 w-3.5" />}
                  </button>
                )}
              </li>
            ))}
          </ul>
        </div>
      </GlassCard>

      {!version ? (
        <GlassCard className="flex min-h-[200px] items-center justify-center">
          <Spinner />
        </GlassCard>
      ) : (
        <>
          <Section title={`Quality report · v${version.version}`} icon={FileText}>
            {report && <QualityReport report={report} />}
          </Section>

          <Section title="Prices" icon={LineChart}>
            <PriceChart versionId={version.id} symbols={report?.symbols ?? []} chartable={version.chartable} />
          </Section>

          <div className="grid gap-5 lg:grid-cols-2">
            <Section title="Where it came from" icon={FileText}>
              <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1.5 text-[12px]">
                <dt className="text-ink/45">Dataset id</dt>
                <dd className="break-all font-mono text-[11px] text-ink/70" title="Hash of the bars and symbols: the same data always has this id">
                  {version.checksum}
                </dd>
                <dt className="text-ink/45">Stored</dt>
                <dd className="num text-ink/70">{bytes(version.byte_size)}</dd>
                <dt className="text-ink/45">Added</dt>
                <dd className="text-ink/70">{new Date(version.created_at).toLocaleString()}</dd>
                <dt className="text-ink/45">Source</dt>
                <dd className="text-ink/70">
                  {version.source?.filename ?? (version.derivation_kind ? `Derived (${version.derivation_kind})` : 'Registered from the command line')}
                </dd>
                {version.derived_from_id && (
                  <>
                    <dt className="text-ink/45">Derived from</dt>
                    <dd className="break-all font-mono text-[11px] text-ink/70">
                      {version.derived_from_id}
                    </dd>
                  </>
                )}
                {version.derivation_params && (
                  <>
                    <dt className="text-ink/45">Lineage</dt>
                    <dd className="text-ink/70 text-[11px]">
                      {typeof version.derivation_params === 'object'
                        ? Object.entries(version.derivation_params)
                            .map(([k, val]) => `${k}: ${val}`)
                            .join(' · ')
                        : String(version.derivation_params)}
                    </dd>
                  </>
                )}
                {settings && (
                  <>
                    <dt className="text-ink/45">Read as</dt>
                    <dd className="text-ink/70">
                      {settings.time_format}, {settings.timezone ? settings.timezone : settings.utc_offset_minutes != null ? formatOffset(settings.utc_offset_minutes) : 'UTC'}, delimiter “{settings.delimiter === '\t' ? 'tab' : settings.delimiter}”
                      {settings.exchange ? `, exchange ${settings.exchange}` : ''}
                      {settings.skip_bad_rows ? ', bad rows skipped' : ''}
                    </dd>
                    <dt className="text-ink/45">Columns</dt>
                    <dd className="text-ink/70">
                      {(['timestamp', 'open', 'high', 'low', 'close', 'volume', 'symbol'] as const)
                        .filter((k) => settings.columns[k])
                        .map((k) => `${k} ← ${settings.columns[k]}`)
                        .join(' · ')}
                      {settings.default_symbol ? ` · symbol = ${settings.default_symbol}` : ''}
                    </dd>
                  </>
                )}
              </dl>
            </Section>

            <Section title="Runs using this version" icon={History}>
              {runs === null ? (
                <Spinner />
              ) : runs.length === 0 ? (
                <p className="text-[12px] text-ink/50">No runs yet.</p>
              ) : (
                <ul className="divide-y divide-ink/[0.05] text-[12px]" aria-label="Runs using this version">
                  {runs.map((r) => (
                    <li key={r.id} className="flex items-center gap-3 py-1.5">
                      <a className="truncate text-accent hover:underline" href={r.job_id ? `/studio?tab=backtests&run=${r.job_id}` : undefined}>
                        {r.strategy ?? r.kind}
                      </a>
                      <Badge tone={r.status === 'succeeded' ? 'profit' : r.status === 'failed' ? 'loss' : 'neutral'}>{r.status}</Badge>
                      {r.total_return != null && <span className={cn('num', tone(r.total_return) === 'profit' ? 'text-profit' : tone(r.total_return) === 'loss' ? 'text-loss' : 'text-ink/60')}>{signedPct(r.total_return)}</span>}
                      <span className="ml-auto text-ink/40">{ago(r.created_at)}</span>
                    </li>
                  ))}
                </ul>
              )}
            </Section>
          </div>
        </>
      )}

      {deriving && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
          <GlassCard className="w-full max-w-lg space-y-4 p-6 shadow-2xl">
            <div className="flex items-center justify-between">
              <h3 className="flex items-center gap-2 text-base font-semibold text-ink">
                <GitFork className="h-4 w-4 text-accent" /> Derive Adjusted Dataset
              </h3>
              <button
                type="button"
                className="btn-ghost p-1"
                onClick={() => setDeriving(false)}
                aria-label="Close"
              >
                <X className="h-4 w-4" />
              </button>
            </div>

            <p className="text-[12px] text-ink/65">
              Apply splits and dividend backward adjustments to <strong className="text-ink">{detail.name}</strong> (v{version?.version}) to create a continuous, unadjusted-gap-free price series.
            </p>

            {deriveError && <ErrorNote>{deriveError}</ErrorNote>}

            <div className="space-y-3 text-[12px]">
              <div>
                <div className="mb-1 flex items-center justify-between">
                  <label className="font-semibold text-ink">Corporate Action Set</label>
                  <button
                    type="button"
                    className="text-[11px] text-accent hover:underline"
                    onClick={() => setCreatingSet(!creatingSet)}
                  >
                    {creatingSet ? 'Select existing set' : '+ Create new set'}
                  </button>
                </div>

                {!creatingSet ? (
                  actionSets.length > 0 ? (
                    <select
                      className="input py-1.5 text-xs"
                      value={selectedSetId}
                      onChange={(e) => setSelectedSetId(e.target.value)}
                    >
                      {actionSets.map((s) => (
                        <option key={s.id} value={s.id}>
                          {s.name} ({s.action_count} actions)
                        </option>
                      ))}
                    </select>
                  ) : (
                    <p className="text-ink/50 text-[11.5px]">No action sets found. Create one below.</p>
                  )
                ) : (
                  <div className="space-y-2 rounded-lg border border-ink/10 bg-canvas/40 p-3">
                    <div>
                      <label className="block text-[11px] text-ink/60">Set Name</label>
                      <input
                        className="input py-1 text-xs"
                        placeholder="e.g. US Equities 2020 Splits & Dividends"
                        value={newSetName}
                        onChange={(e) => setNewSetName(e.target.value)}
                      />
                    </div>
                    <div>
                      <label className="block text-[11px] text-ink/60">Description (optional)</label>
                      <input
                        className="input py-1 text-xs"
                        placeholder="e.g. Backward adjustments for tech stocks"
                        value={newSetDesc}
                        onChange={(e) => setNewSetDesc(e.target.value)}
                      />
                    </div>
                    <div className="grid grid-cols-2 gap-2">
                      <div>
                        <label className="block text-[11px] text-ink/60">Symbol</label>
                        <input
                          className="input py-1 text-xs"
                          placeholder="AAPL"
                          value={newActionSymbol}
                          onChange={(e) => setNewActionSymbol(e.target.value)}
                        />
                      </div>
                      <div>
                        <label className="block text-[11px] text-ink/60">Action Kind</label>
                        <select
                          className="input py-1 text-xs"
                          value={newActionKind}
                          onChange={(e) => setNewActionKind(e.target.value as 'split' | 'dividend')}
                        >
                          <option value="split">Split</option>
                          <option value="dividend">Cash Dividend</option>
                        </select>
                      </div>
                    </div>
                    <div className="grid grid-cols-2 gap-2">
                      <div>
                        <label className="block text-[11px] text-ink/60">Ex-Date (YYYY-MM-DD)</label>
                        <input
                          className="input py-1 text-xs"
                          type="date"
                          value={newActionExDate}
                          onChange={(e) => setNewActionExDate(e.target.value)}
                        />
                      </div>
                      {newActionKind === 'split' ? (
                        <div className="flex gap-2 items-center">
                          <div className="flex-1">
                            <label className="block text-[11px] text-ink/60">From</label>
                            <input
                              className="input py-1 text-xs"
                              type="number"
                              min={1}
                              value={newActionSplitFrom}
                              onChange={(e) => setNewActionSplitFrom(Number(e.target.value))}
                            />
                          </div>
                          <span className="mt-3 text-ink/40">:</span>
                          <div className="flex-1">
                            <label className="block text-[11px] text-ink/60">To</label>
                            <input
                              className="input py-1 text-xs"
                              type="number"
                              min={1}
                              value={newActionSplitTo}
                              onChange={(e) => setNewActionSplitTo(Number(e.target.value))}
                            />
                          </div>
                        </div>
                      ) : (
                        <div>
                          <label className="block text-[11px] text-ink/60">Dividend per Share</label>
                          <input
                            className="input py-1 text-xs"
                            placeholder="0.205"
                            value={newActionDividend}
                            onChange={(e) => setNewActionDividend(e.target.value)}
                          />
                        </div>
                      )}
                    </div>
                  </div>
                )}
              </div>

              <div className="space-y-1.5 pt-1">
                <label className="font-semibold text-ink">Adjustment Options</label>
                <div className="flex gap-4">
                  <label className="flex items-center gap-1.5 text-ink/80 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={adjustSplits}
                      onChange={(e) => setAdjustSplits(e.target.checked)}
                    />
                    Adjust Splits
                  </label>
                  <label className="flex items-center gap-1.5 text-ink/80 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={adjustDividends}
                      onChange={(e) => setAdjustDividends(e.target.checked)}
                    />
                    Adjust Dividends
                  </label>
                </div>
              </div>

              <div>
                <label className="block font-semibold text-ink">Target Dataset Name</label>
                <input
                  className="input py-1 text-xs mt-1"
                  placeholder={`Leave empty to add new version to "${detail.name}"`}
                  value={targetDatasetName}
                  onChange={(e) => setTargetDatasetName(e.target.value)}
                />
                <span className="text-[11px] text-ink/40">
                  Optional. If omitted, the new adjusted version is appended to the current dataset.
                </span>
              </div>
            </div>

            <div className="flex justify-end gap-2 pt-2">
              <button
                type="button"
                className="btn-ghost text-xs"
                onClick={() => setDeriving(false)}
                disabled={derivingBusy}
              >
                Cancel
              </button>
              <button
                type="button"
                className="btn-primary text-xs"
                onClick={handleDerive}
                disabled={derivingBusy}
              >
                {derivingBusy ? <Spinner className="h-3.5 w-3.5" /> : <GitFork className="h-3.5 w-3.5" />}
                Derive
              </button>
            </div>
          </GlassCard>
        </div>
      )}
    </div>
  );
}
