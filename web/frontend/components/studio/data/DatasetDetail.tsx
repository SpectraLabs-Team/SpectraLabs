'use client';

import { useCallback, useEffect, useState } from 'react';
import { Check, FileText, History, LineChart, Pencil, Trash2, X } from 'lucide-react';
import { ApiError, detailErrors } from '@/lib/api';
import { deleteVersion, getDataset, getVersion, getVersionRuns, updateDataset, type DatasetDetail as Detail, type VersionDetail, type VersionRun } from '@/lib/datasets';
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
            {versionId && (
              <button type="button" className="btn-primary" onClick={() => onBacktest(versionId)}>
                <LineChart className="h-3.5 w-3.5" /> Backtest this dataset
              </button>
            )}
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
                <dd className="text-ink/70">{version.source?.filename ?? 'Registered from the command line'}</dd>
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
    </div>
  );
}
