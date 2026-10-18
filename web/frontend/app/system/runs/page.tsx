import { Clock3, Database } from 'lucide-react';

export default function SystemRunsPage() {
  return <div className="mx-auto max-w-7xl space-y-7 p-5 sm:p-8">
    <header><p className="mb-2 text-xs font-semibold uppercase tracking-[0.2em] text-accent-primary">System</p><h1 className="text-3xl font-semibold tracking-tight">Run history</h1><p className="mt-2 text-sm text-foreground-muted">Reproducibility and audit trail for risk calculations.</p></header>
    <section className="rounded-xl border border-border bg-surface p-8 sm:p-12">
      <div className="mx-auto max-w-lg text-center"><div className="mx-auto flex h-11 w-11 items-center justify-center rounded-full bg-surface-elevated text-foreground-muted"><Clock3 size={20} /></div><h2 className="mt-4 font-semibold">Run persistence is not connected</h2><p className="mt-2 text-sm leading-6 text-foreground-muted">The calculation API returns reproducibility metadata for each run, but this project does not yet store runs. Results disappear when you leave the calculation screen.</p><div className="mt-5 inline-flex items-center gap-2 rounded-lg border border-border px-3 py-2 text-xs text-foreground-subtle"><Database size={14} /> No run database configured</div></div>
    </section>
  </div>;
}
