'use client';

import Link from 'next/link';
import { useEffect, useState, type FormEvent, type ReactNode } from 'react';
import { Activity, ArrowRight, Check, KeyRound, LogOut, MonitorSmartphone, Pencil, Trash2, User, X } from 'lucide-react';
import { ACTIVITY_RETENTION_DAYS_CLIENT } from '@/lib/activityClient';
import { AccountSecurity } from '@/components/studio/AccountSecurity';
import { AuthenticatorApp } from './AuthenticatorApp';
import { GlassCard, Spinner } from '@/components/ui/primitives';
import { checkPasswordRules, cleanName, displayName, initials, isPasswordValid, MAX_NAME, MIN_NAME, useAuth, type Role } from '@/lib/auth';
import { cn } from '@/lib/utils';

type Profile = {
  email: string;
  name: string;
  role: Role;
  createdAt: number;
  twoFactor: boolean;
  twoFactorEnforced: boolean;
  passwordChangedAt: number | null;
};

const dateFmt = new Intl.DateTimeFormat(undefined, { year: 'numeric', month: 'long', day: 'numeric' });

const input =
  'w-full rounded-xl border border-ink/15 bg-canvas/70 px-3 py-2 text-sm text-ink outline-none transition placeholder:text-ink/35 focus:border-cyan-500 focus:ring-2 focus:ring-cyan-500/25';

async function api<T = any>(url: string, method: string, body?: unknown): Promise<T> {
  const res = await fetch(url, {
    method,
    credentials: 'same-origin',
    cache: 'no-store',
    headers: body ? { 'Content-Type': 'application/json' } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok || !data.ok) throw new Error(data.error || 'Something went wrong. Try again.');
  return data;
}

function Section({ icon, title, desc, children, tone }: { icon: ReactNode; title: string; desc?: string; children: ReactNode; tone?: 'danger' }) {
  return (
    <GlassCard as="section" className={cn('p-5 sm:p-6', tone === 'danger' && 'border-loss/25')}>
      <h2 className={cn('flex items-center gap-2 text-sm font-semibold', tone === 'danger' ? 'text-loss' : 'text-ink')}>
        <span className={tone === 'danger' ? 'text-loss' : 'text-accent'}>{icon}</span>
        {title}
      </h2>
      {desc && <p className="mt-1 max-w-xl text-xs leading-5 text-ink/55">{desc}</p>}
      <div className="mt-4">{children}</div>
    </GlassCard>
  );
}

function Feedback({ error, notice }: { error?: string; notice?: string }) {
  if (error) return <p role="alert" className="mt-3 text-[12px] text-loss">{error}</p>;
  if (notice) return <p role="status" className="mt-3 text-[12px] text-profit">{notice}</p>;
  return null;
}

// ── Profile ─────────────────────────────────────────────────────────────────
function ProfileSection({ profile, onSaved }: { profile: Profile; onSaved: (p: Profile) => void }) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(profile.name);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const valid = cleanName(name) !== '';

  const save = async (e: FormEvent) => {
    e.preventDefault();
    if (!valid || busy) return;
    setBusy(true);
    setError('');
    try {
      const d = await api<{ profile: Profile }>('/api/auth/account', 'PATCH', { name });
      onSaved(d.profile);
      setEditing(false);
      setNotice('Name updated.');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not update your name.');
    } finally {
      setBusy(false);
    }
  };

  const rows: [string, ReactNode][] = [
    ['Email', <span key="e" className="font-mono text-[13px]">{profile.email}</span>],
    [
      'Role',
      <span key="r" className={cn('rounded-full px-2 py-0.5 text-[11px] font-semibold', profile.role === 'admin' ? 'bg-cyan-500/15 text-cyan-700 dark:text-cyan-300' : 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300')}>
        {profile.role === 'admin' ? 'Admin' : 'User'}
      </span>,
    ],
    ['Member since', dateFmt.format(new Date(profile.createdAt))],
    ['Password last changed', profile.passwordChangedAt ? dateFmt.format(new Date(profile.passwordChangedAt)) : 'Never'],
  ];

  return (
    <Section icon={<User className="h-4 w-4" />} title="Profile">
      <div className="flex flex-wrap items-center gap-4">
        <span aria-hidden className="flex h-14 w-14 shrink-0 items-center justify-center rounded-2xl bg-accent-gradient font-mono text-lg font-bold text-onaccent shadow-glow">
          {initials(profile)}
        </span>
        {editing ? (
          <form onSubmit={save} className="flex min-w-[240px] flex-1 flex-wrap items-center gap-2">
            <label className="min-w-[200px] flex-1">
              <span className="sr-only">Full name</span>
              <input autoFocus value={name} maxLength={MAX_NAME} onChange={(e) => setName(e.target.value)} className={cn(input, !valid && 'border-loss/60')} />
            </label>
            <button type="submit" disabled={!valid || busy} className="btn-primary px-3 py-2 text-xs">
              {busy ? <Spinner className="h-3.5 w-3.5" /> : <Check className="h-3.5 w-3.5" />} Save
            </button>
            <button type="button" onClick={() => (setEditing(false), setName(profile.name), setError(''))} className="btn-ghost border border-ink/10 px-3 py-2 text-xs">
              <X className="h-3.5 w-3.5" /> Cancel
            </button>
            {!valid && <p className="w-full text-[11.5px] text-loss">At least {MIN_NAME} characters.</p>}
          </form>
        ) : (
          <div className="min-w-0 flex-1">
            <p className="flex items-center gap-2 text-lg font-semibold text-ink">
              <span className="truncate">{displayName(profile)}</span>
              <button type="button" onClick={() => (setEditing(true), setNotice(''))} aria-label="Edit name" className="rounded-md p-1 text-ink/45 transition hover:bg-ink/[0.06] hover:text-ink">
                <Pencil className="h-3.5 w-3.5" />
              </button>
            </p>
            <p className="text-[12.5px] text-ink/50">{profile.email}</p>
          </div>
        )}
      </div>
      <dl className="mt-5 grid gap-x-8 gap-y-3 border-t border-ink/[0.06] pt-4 text-[12.5px] sm:grid-cols-2">
        {rows.map(([k, v]) => (
          <div key={k} className="flex items-center justify-between gap-3 sm:block">
            <dt className="text-ink/50">{k}</dt>
            <dd className="text-ink/90 sm:mt-0.5">{v}</dd>
          </div>
        ))}
      </dl>
      <p className="mt-3 text-[11px] text-ink/40">Your email is your sign-in and can&apos;t be changed here.</p>
      <Feedback error={error} notice={notice} />
    </Section>
  );
}

// ── Change password ─────────────────────────────────────────────────────────
function ChangePassword({ onChanged }: { onChanged: () => void }) {
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [confirm, setConfirm] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const rules = checkPasswordRules(next);
  const match = confirm.length > 0 && confirm === next;
  const ok = current.length > 0 && isPasswordValid(next) && match;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!ok || busy) return;
    setBusy(true);
    setError('');
    setNotice('');
    try {
      await api('/api/auth/password', 'POST', { currentPassword: current, newPassword: next });
      setCurrent('');
      setNext('');
      setConfirm('');
      setNotice('Password changed. You’ve been signed out on every other device.');
      onChanged();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not change the password.');
    } finally {
      setBusy(false);
    }
  };

  const rule = (okay: boolean, label: string) => (
    <span className={cn('flex items-center gap-1.5', okay ? 'font-medium text-emerald-600 dark:text-emerald-400' : 'text-ink/40')}>
      <span className={cn('h-1.5 w-1.5 rounded-full', okay ? 'bg-emerald-500' : 'bg-ink/30')} />
      {label}
    </span>
  );

  return (
    <Section icon={<KeyRound className="h-4 w-4" />} title="Change password" desc="Changing your password signs you out on every other device. You stay signed in here.">
      <form onSubmit={submit} className="grid gap-3 sm:max-w-md">
        <label className="block">
          <span className="text-[12px] font-medium text-ink/70">Current password</span>
          <input type="password" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} className={cn(input, 'mt-1.5')} />
        </label>
        <label className="block">
          <span className="text-[12px] font-medium text-ink/70">New password</span>
          <input type="password" autoComplete="new-password" value={next} onChange={(e) => setNext(e.target.value)} className={cn(input, 'mt-1.5')} />
        </label>
        <div className="grid grid-cols-2 gap-1.5 rounded-lg border border-ink/[0.06] bg-ink/[0.02] p-2.5 text-[11px]">
          {rule(rules.length, '8+ characters')}
          {rule(rules.letter, '1 letter (a-z)')}
          {rule(rules.number, '1 number (0-9)')}
          {rule(rules.special, '1 special char (!@#$)')}
        </div>
        <label className="block">
          <span className="text-[12px] font-medium text-ink/70">Confirm new password</span>
          <input type="password" autoComplete="new-password" value={confirm} onChange={(e) => setConfirm(e.target.value)} className={cn(input, 'mt-1.5', confirm && !match && 'border-loss/60')} />
        </label>
        {confirm && (
          <p className={cn('-mt-1 flex items-center gap-1.5 text-[12px] font-medium', match ? 'text-emerald-600 dark:text-emerald-400' : 'text-rose-500')}>
            {match ? <><Check className="h-3.5 w-3.5" /> Passwords match</> : 'Passwords do not match'}
          </p>
        )}
        <div>
          <button type="submit" disabled={!ok || busy} className="btn-primary px-4 py-2 text-xs">
            {busy ? <Spinner className="h-3.5 w-3.5" /> : <KeyRound className="h-3.5 w-3.5" />} Update password
          </button>
        </div>
      </form>
      <Feedback error={error} notice={notice} />
    </Section>
  );
}

// ── Sessions & devices ──────────────────────────────────────────────────────
type SessionRow = {
  id: string;
  current: boolean;
  createdAt: number;
  lastSeenAt: number;
  ip: string | null;
  device: { browser: string; os: string; kind: 'desktop' | 'mobile' | 'tablet'; label: string };
  deviceFirstSeenAt: number | null;
};

const ago = (ms: number) => {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 90) return 'just now';
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86_400) return `${Math.round(s / 3600)} h ago`;
  return new Date(ms).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
};

function Sessions() {
  const [rows, setRows] = useState<SessionRow[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');

  const load = () =>
    api<{ sessions: SessionRow[] }>('/api/auth/sessions', 'GET')
      .then((d) => setRows(d.sessions))
      .catch((err) => setError(err instanceof Error ? err.message : 'Could not load your devices.'));
  useEffect(() => {
    load();
  }, []);

  const run = async (key: string, url: string, done: (ended: number) => string) => {
    setBusy(key);
    setError('');
    setNotice('');
    try {
      const d = await api<{ ended: number }>(url, 'DELETE');
      setNotice(done(d.ended));
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not sign out.');
    } finally {
      setBusy(null);
    }
  };

  const others = rows?.filter((r) => !r.current).length ?? 0;

  return (
    <Section
      icon={<MonitorSmartphone className="h-4 w-4" />}
      title="Devices"
      desc="Everywhere you're signed in. Don't recognise one? Sign it out, then change your password. You get an email when a new device signs in."
    >
      {!rows && !error && <Spinner className="h-4 w-4" />}
      {rows && (
        <ul className="divide-y divide-ink/[0.06] rounded-xl border border-ink/[0.08]">
          {rows.map((r) => (
            <li key={r.id} className="flex flex-wrap items-center gap-x-4 gap-y-1 px-3.5 py-3">
              <MonitorSmartphone className={cn('h-4 w-4 shrink-0', r.current ? 'text-accent' : 'text-ink/40')} />
              <div className="min-w-0 flex-1">
                <p className="text-[13px] font-medium text-ink">
                  {r.device.label}
                  {r.current && <span className="ml-2 rounded-full bg-accent/10 px-2 py-0.5 text-[10.5px] font-semibold text-accent">This device</span>}
                </p>
                <p className="mt-0.5 text-[11.5px] text-ink/50">
                  {r.ip ?? 'unknown IP'} · {r.current ? 'active now' : `last active ${ago(r.lastSeenAt)}`} · signed in {ago(r.createdAt)}
                  {r.deviceFirstSeenAt ? ` · first seen ${new Date(r.deviceFirstSeenAt).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' })}` : ''}
                </p>
              </div>
              {!r.current && (
                <button
                  type="button"
                  className="btn-ghost border border-ink/10 hover:text-loss"
                  disabled={busy !== null}
                  onClick={() => run(r.id, `/api/auth/sessions?id=${encodeURIComponent(r.id)}`, () => `Signed out ${r.device.label}.`)}
                >
                  {busy === r.id ? <Spinner className="h-3.5 w-3.5" /> : <LogOut className="h-3.5 w-3.5" />} Sign out
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {rows && others > 0 && (
        <button
          type="button"
          onClick={() => run('all', '/api/auth/sessions', (n) => `Signed out of ${n} other device${n === 1 ? '' : 's'}. This one stays signed in.`)}
          disabled={busy !== null}
          className="btn-ghost mt-3 border border-ink/10 px-3 py-2 text-xs"
        >
          {busy === 'all' ? <Spinner className="h-3.5 w-3.5" /> : <LogOut className="h-3.5 w-3.5" />} Sign out of all other devices
        </button>
      )}
      {rows && others === 0 && <p className="mt-3 text-[12px] text-ink/45">You&apos;re only signed in here.</p>}
      <Feedback error={error} notice={notice} />
    </Section>
  );
}

// ── Delete account ──────────────────────────────────────────────────────────
function DeleteAccount({ profile }: { profile: Profile }) {
  const { logout } = useAuth();
  const [open, setOpen] = useState(false);
  const [password, setPassword] = useState('');
  const [typed, setTyped] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const admin = profile.role === 'admin';
  const confirmed = typed.trim().toLowerCase() === 'delete';

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!password || !confirmed || busy) return;
    setBusy(true);
    setError('');
    try {
      await api('/api/auth/account', 'DELETE', { password });
      // Remove this account's saved portfolio and paper settings from this browser too.
      try {
        localStorage.removeItem(`spectra-portfolio-${profile.email}`);
        localStorage.removeItem(`spectra-paper-${profile.email}`);
      } catch {}
      await logout('/');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not delete the account.');
      setBusy(false);
    }
  };

  return (
    <Section icon={<Trash2 className="h-4 w-4" />} title="Delete account" tone="danger" desc="Permanently removes your account, your personal workspace with its experiments, runs and their history, and any team workspace where you are the only member. Export research you want to keep first. This can't be undone.">
      {admin ? (
        <p className="text-[12.5px] text-ink/55">The admin account can&apos;t be deleted from here, so the platform always keeps an administrator.</p>
      ) : !open ? (
        <button type="button" onClick={() => setOpen(true)} className="inline-flex items-center gap-1.5 rounded-lg border border-loss/35 px-3 py-2 text-xs font-semibold text-loss transition hover:bg-loss/10">
          <Trash2 className="h-3.5 w-3.5" /> Delete my account…
        </button>
      ) : (
        <form onSubmit={submit} className="grid gap-3 rounded-xl border border-loss/25 bg-loss/[0.04] p-4 sm:max-w-md">
          <label className="block">
            <span className="text-[12px] font-medium text-ink/70">Your password</span>
            <input type="password" autoComplete="current-password" autoFocus value={password} onChange={(e) => setPassword(e.target.value)} className={cn(input, 'mt-1.5')} />
          </label>
          <label className="block">
            <span className="text-[12px] font-medium text-ink/70">
              Type <span className="font-mono font-semibold text-loss">delete</span> to confirm
            </span>
            <input value={typed} onChange={(e) => setTyped(e.target.value)} className={cn(input, 'mt-1.5')} />
          </label>
          <div className="flex gap-2">
            <button type="button" onClick={() => (setOpen(false), setPassword(''), setTyped(''), setError(''))} className="btn-ghost border border-ink/10 px-3 py-2 text-xs">
              Cancel
            </button>
            <button type="submit" disabled={!password || !confirmed || busy} className="inline-flex items-center gap-1.5 rounded-lg bg-loss px-3 py-2 text-xs font-bold text-white transition hover:brightness-110 disabled:opacity-50">
              {busy ? <Spinner className="h-3.5 w-3.5" /> : <Trash2 className="h-3.5 w-3.5" />} Permanently delete
            </button>
          </div>
        </form>
      )}
      <Feedback error={error} />
    </Section>
  );
}

// ── Page ────────────────────────────────────────────────────────────────────
/** What usage data the platform keeps about this account (see ActivityTracker and lib/server/activity.ts). */
function UsageData() {
  return (
    <Section
      icon={<Activity className="h-4 w-4" />}
      title="Usage data we keep"
      desc="So platform admins can see how Spectra is used and spot problems, we keep a small amount of activity data about your account."
    >
      <ul className="mt-3 list-disc space-y-1.5 pl-5 text-[13px] leading-relaxed text-ink/75" data-testid="usage-disclosure">
        <li>Sign-ins and your last active time.</li>
        <li>
          Approximate active time per day. While a Spectra tab is visible and you have used it in the last few minutes, the page
          sends a signal about once a minute. Time with the tab hidden or idle is not counted, so the figure is an estimate.
        </li>
        <li>Counts of what you run, such as simulations, backtests and paper orders, taken from your workspace history.</li>
        <li>Your personal run history lists the runs you started. You can remove individual runs from your own view at any time without deleting them from the workspace.</li>
        <li>We do not record the pages you view, your clicks or what you type.</li>
        <li>Daily activity totals are deleted after {ACTIVITY_RETENTION_DAYS_CLIENT} days. Deleting your account removes them immediately.</li>
        <li>
          Security records, such as sign-ins and admin actions, are kept permanently in a tamper-evident log, with the IP address and
          browser used. They stay after you delete your account.
        </li>
        <li>Admins can see these figures for your account, and every time they open them is recorded in the audit log.</li>
      </ul>
    </Section>
  );
}

export function AccountPage() {
  const { user, login } = useAuth();
  const [profile, setProfile] = useState<Profile | null>(null);
  const [error, setError] = useState('');

  const load = () =>
    api<{ profile: Profile }>('/api/auth/account', 'GET')
      .then((d) => setProfile(d.profile))
      .catch((e) => setError(e instanceof Error ? e.message : 'Could not load your account.'));

  useEffect(() => {
    load();
  }, []);

  return (
    <>
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <p className="font-mono text-[11.5px] font-medium uppercase tracking-[0.22em] text-ink/60">Account</p>
          <h1 className="mt-3 text-3xl font-bold tracking-tight text-ink sm:text-[2.15rem]">Your profile &amp; security</h1>
        </div>
        <div className="flex flex-wrap gap-2">
          {user?.role === 'admin' && (
            <Link href="/admin" className="btn-ghost border border-ink/10 px-3 py-2 text-xs">
              Admin Dashboard
            </Link>
          )}
          <Link href="/studio" className="btn-primary px-4 py-2 text-xs">
            Open Quant Studio <ArrowRight className="h-3.5 w-3.5" />
          </Link>
        </div>
      </div>

      {error && <p className="mt-6 text-sm text-loss">{error}</p>}
      {!profile && !error && (
        <div className="mt-10 flex justify-center">
          <Spinner className="h-5 w-5" />
        </div>
      )}
      {profile && (
        <div className="mt-8 grid gap-5">
          <ProfileSection
            profile={profile}
            onSaved={(p) => {
              setProfile(p);
              // Update the header/Studio name everywhere (and in other tabs).
              login({ email: p.email, role: p.role, name: p.name });
            }}
          />
          <ChangePassword onChanged={load} />
          <AccountSecurity />
          <AuthenticatorApp />
          <Sessions />
          <UsageData />
          <DeleteAccount profile={profile} />
        </div>
      )}
    </>
  );
}
