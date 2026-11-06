import Link from 'next/link';
import { ShieldCheck, Lock, Clock, Database, FileText, Trash2, ArrowLeft } from 'lucide-react';
import { GlassCard } from '@/components/ui/primitives';
import { ACTIVITY_RETENTION_DAYS_CLIENT } from '@/lib/activityClient';

export function Privacy() {
  return (
    <div className="mx-auto max-w-4xl space-y-12">
      <div className="space-y-4">
        <Link
          href="/studio"
          className="inline-flex items-center gap-1.5 text-xs font-semibold text-accent hover:brightness-110"
        >
          <ArrowLeft className="h-3.5 w-3.5" /> Back to Studio
        </Link>
        <div>
          <p className="font-mono text-xs font-semibold uppercase tracking-widest text-accent">Compliance &amp; Governance</p>
          <h1 className="mt-2 text-3xl font-extrabold tracking-tight text-ink sm:text-4xl">
            Privacy &amp; Data Retention Policy
          </h1>
          <p className="mt-3 text-base text-ink/70">
            Spectra Labs is built on evidence-first quantitative principles and respect for your research privacy. This document outlines exactly what personal and workspace data we store, retention timelines, and how data is permanently deleted.
          </p>
        </div>
      </div>

      {/* Summary Highlights */}
      <div className="grid gap-4 sm:grid-cols-3">
        <GlassCard className="p-5">
          <div className="flex items-center gap-2 text-accent">
            <Clock className="h-4 w-4" />
            <h3 className="text-sm font-bold text-ink">400-Day Activity Cap</h3>
          </div>
          <p className="mt-2 text-xs leading-relaxed text-ink/65">
            Aggregated active-time totals are purged automatically after {ACTIVITY_RETENTION_DAYS_CLIENT} days. No clickstreams, page URLs, or keystrokes are recorded.
          </p>
        </GlassCard>

        <GlassCard className="p-5">
          <div className="flex items-center gap-2 text-accent">
            <Lock className="h-4 w-4" />
            <h3 className="text-sm font-bold text-ink">Tamper-Proof Audit Log</h3>
          </div>
          <p className="mt-2 text-xs leading-relaxed text-ink/65">
            Security and administrative events are stored in a cryptographic, hash-chained log to detect any unauthorized modifications.
          </p>
        </GlassCard>

        <GlassCard className="p-5">
          <div className="flex items-center gap-2 text-accent">
            <Trash2 className="h-4 w-4" />
            <h3 className="text-sm font-bold text-ink">Complete Deletion</h3>
          </div>
          <p className="mt-2 text-xs leading-relaxed text-ink/65">
            Deleting your account immediately purges personal workspaces, strategy drafts, active sessions, and activity aggregates.
          </p>
        </GlassCard>
      </div>

      {/* Data Retention Lifecycle Table */}
      <section className="space-y-4">
        <div className="flex items-center gap-2">
          <Database className="h-5 w-5 text-accent" />
          <h2 className="text-xl font-bold text-ink">Data Retention Lifecycle</h2>
        </div>
        <p className="text-xs text-ink/60">
          Our retention schedule is enforced programmatically by background workers and automated database routines.
        </p>

        <div className="overflow-x-auto rounded-xl border border-ink/10 bg-panel/30">
          <table className="w-full text-left text-xs">
            <thead className="border-b border-ink/10 bg-ink/[0.03] font-mono uppercase tracking-wider text-ink/60">
              <tr>
                <th className="px-4 py-3">Category</th>
                <th className="px-4 py-3">Storage Location</th>
                <th className="px-4 py-3">Retention Period</th>
                <th className="px-4 py-3">Enforcement Mechanism</th>
                <th className="px-4 py-3">Upon Account Deletion</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-ink/[0.06] text-ink/80">
              <tr>
                <td className="px-4 py-3 font-semibold text-ink">Account Profile</td>
                <td className="px-4 py-3 font-mono text-[11px] text-ink/60">users</td>
                <td className="px-4 py-3">Until account deletion</td>
                <td className="px-4 py-3">User account management</td>
                <td className="px-4 py-3 text-profit font-medium">Deleted immediately</td>
              </tr>
              <tr>
                <td className="px-4 py-3 font-semibold text-ink">Daily Activity Totals</td>
                <td className="px-4 py-3 font-mono text-[11px] text-ink/60">user_activity_daily</td>
                <td className="px-4 py-3 font-bold text-accent">{ACTIVITY_RETENTION_DAYS_CLIENT} days</td>
                <td className="px-4 py-3">Automated 6-hour purge worker</td>
                <td className="px-4 py-3 text-profit font-medium">Deleted immediately (CASCADE)</td>
              </tr>
              <tr>
                <td className="px-4 py-3 font-semibold text-ink">Active Sessions</td>
                <td className="px-4 py-3 font-mono text-[11px] text-ink/60">sessions</td>
                <td className="px-4 py-3">Max 7 days (or 24h idle)</td>
                <td className="px-4 py-3">Session housekeeping cron</td>
                <td className="px-4 py-3 text-profit font-medium">Invalidated &amp; purged</td>
              </tr>
              <tr>
                <td className="px-4 py-3 font-semibold text-ink">Security Audit Log</td>
                <td className="px-4 py-3 font-mono text-[11px] text-ink/60">audit_events</td>
                <td className="px-4 py-3 font-semibold text-amber-500">Permanent</td>
                <td className="px-4 py-3">Append-only DB triggers &amp; hash chain</td>
                <td className="px-4 py-3 text-ink/50">Retained for compliance &amp; security</td>
              </tr>
              <tr>
                <td className="px-4 py-3 font-semibold text-ink">Runs, Notes &amp; Drafts</td>
                <td className="px-4 py-3 font-mono text-[11px] text-ink/60">runs, strategy_drafts</td>
                <td className="px-4 py-3">Until workspace or run deletion</td>
                <td className="px-4 py-3">User workspace action</td>
                <td className="px-4 py-3 text-profit font-medium">Deleted with workspace</td>
              </tr>
              <tr>
                <td className="px-4 py-3 font-semibold text-ink">Staged Uploads</td>
                <td className="px-4 py-3 font-mono text-[11px] text-ink/60">uploads, staging</td>
                <td className="px-4 py-3">24 hours</td>
                <td className="px-4 py-3">Automatic TTL expiry sweep</td>
                <td className="px-4 py-3 text-profit font-medium">Swept on expiry</td>
              </tr>
              <tr>
                <td className="px-4 py-3 font-semibold text-ink">Database &amp; Store Backups</td>
                <td className="px-4 py-3 font-mono text-[11px] text-ink/60">Encrypted backup store</td>
                <td className="px-4 py-3">14 daily / 90 weekly</td>
                <td className="px-4 py-3">Nightly backup rotation scripts</td>
                <td className="px-4 py-3 text-ink/50">Aged out via retention cycle</td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>

      {/* Audit Log Rationale & Transparency */}
      <section className="space-y-3">
        <div className="flex items-center gap-2">
          <ShieldCheck className="h-5 w-5 text-accent" />
          <h2 className="text-xl font-bold text-ink">Why the Audit Log is Permanent</h2>
        </div>
        <div className="rounded-xl border border-ink/10 bg-panel/30 p-5 text-xs leading-relaxed text-ink/75 space-y-2">
          <p>
            The audit log records sensitive security events such as sign-ins, two-factor authentication changes, password updates, and administrative role modifications.
          </p>
          <p>
            Each entry is cryptographically anchored in a sequential SHA-256 hash chain with database triggers that reject any <code>UPDATE</code>, <code>DELETE</code>, or <code>TRUNCATE</code> operations. This prevents even database operators or compromised administrative credentials from quietly modifying past access records.
          </p>
        </div>
      </section>

      {/* Legal & Regulatory Review Notes */}
      <section className="space-y-3">
        <div className="flex items-center gap-2">
          <FileText className="h-5 w-5 text-accent" />
          <h2 className="text-xl font-bold text-ink">Legal Review &amp; Compliance Notes (M33 Alignment)</h2>
        </div>
        <div className="rounded-xl border border-ink/10 bg-panel/30 p-5 text-xs leading-relaxed text-ink/70 space-y-2">
          <p>
            <strong>Storage Limitation (GDPR Art. 5(1)(e)):</strong> Personal telemetry is minimized to active-minute approximations and retained strictly for the {ACTIVITY_RETENTION_DAYS_CLIENT}-day operational window before being permanently purged.
          </p>
          <p>
            <strong>Legitimate Interests &amp; Legal Compliance:</strong> Security audit logs are maintained permanently under legitimate interests for fraud detection, unauthorized access auditing, and regulatory compliance obligations.
          </p>
          <p>
            <strong>Policy Approvals:</strong> Formal data retention policy (M28 Decisions 1–6) approved on October 30, 2026. Statutory legal review (GDPR Art. 5(1)(e) &amp; UK DPA 2018) completed and signed on November 6, 2026.
          </p>
        </div>
      </section>
    </div>
  );
}
