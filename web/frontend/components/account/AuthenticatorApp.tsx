'use client';

import { useEffect, useState, type FormEvent } from 'react';
import { Copy, Download, KeyRound, Smartphone } from 'lucide-react';
import { useReauth } from '@/components/auth/useReauth';
import { GlassCard, Spinner } from '@/components/ui/primitives';

type Status = { totp: 'off' | 'pending' | 'on'; totpSince: number | null; recoveryCodesLeft: number };
type Enrol = { secret: string; qrSvg: string };

/**
 * Account → Authenticator app: set up (QR code + first code), recovery codes (shown once),
 * regenerate them, or remove the app. Starting setup, regenerating and removing ask for a fresh
 * "confirm it's you".
 */
export function AuthenticatorApp() {
  const { call, dialog } = useReauth();
  const [status, setStatus] = useState<Status | null>(null);
  const [enrol, setEnrol] = useState<Enrol | null>(null);
  const [code, setCode] = useState('');
  const [codes, setCodes] = useState<string[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [copied, setCopied] = useState(false);

  const load = () => call<{ status: Status }>('/api/auth/totp').then((d) => setStatus(d.status)).catch((e) => setError(e.message));
  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const run = async (key: string, fn: () => Promise<void>) => {
    setBusy(key);
    setError('');
    try {
      await fn();
    } catch (e) {
      if (!(e instanceof Error && e.message === 'Cancelled.')) setError(e instanceof Error ? e.message : 'Something went wrong.');
    } finally {
      setBusy(null);
    }
  };

  const start = () =>
    run('start', async () => {
      const d = await call<Enrol>('/api/auth/totp', 'POST', { action: 'start' });
      setEnrol({ secret: d.secret, qrSvg: d.qrSvg });
      setCode('');
    });

  const confirm = (e: FormEvent) => {
    e.preventDefault();
    run('confirm', async () => {
      const d = await call<{ recoveryCodes: string[]; status: Status }>('/api/auth/totp', 'POST', { action: 'confirm', code: code.trim() });
      setEnrol(null);
      setCodes(d.recoveryCodes);
      setStatus(d.status);
    });
  };

  const regenerate = () =>
    run('regen', async () => {
      const d = await call<{ recoveryCodes: string[]; status: Status }>('/api/auth/totp', 'POST', { action: 'recovery-codes' });
      setCodes(d.recoveryCodes);
      setStatus(d.status);
    });

  const remove = () =>
    run('remove', async () => {
      const d = await call<{ status: Status }>('/api/auth/totp', 'DELETE');
      setStatus(d.status);
      setCodes(null);
    });

  const codesText = codes ? `Spectra Labs recovery codes (each works once)\n\n${codes.join('\n')}\n` : '';

  return (
    <GlassCard as="section" className="p-5 sm:p-6">
      {dialog}
      <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
        <Smartphone className="h-4 w-4 text-accent" /> Authenticator app
      </h2>
      <p className="mt-1 max-w-2xl text-xs leading-5 text-ink/55">
        Use a code from an app like Google Authenticator, 1Password or Authy instead of an emailed code. It works offline and can&apos;t be intercepted in your inbox.
      </p>

      {!status && !error && <Spinner className="mt-4 h-4 w-4" />}

      {codes && (
        <div className="mt-4 rounded-xl border border-caution/30 bg-caution/[0.06] p-4">
          <p className="text-[13px] font-semibold text-ink">Save your recovery codes now</p>
          <p className="mt-1 text-xs leading-5 text-ink/60">If you lose your phone, each code signs you in once. They won&apos;t be shown again.</p>
          <ol className="mt-3 grid grid-cols-2 gap-x-6 gap-y-1 font-mono text-[13px] text-ink sm:grid-cols-5">
            {codes.map((c) => (
              <li key={c}>{c}</li>
            ))}
          </ol>
          <div className="mt-3 flex flex-wrap gap-2">
            <button
              type="button"
              className="btn-ghost border border-ink/10"
              onClick={() => navigator.clipboard?.writeText(codesText).then(() => (setCopied(true), setTimeout(() => setCopied(false), 1500)))}
            >
              <Copy className="h-3.5 w-3.5" /> {copied ? 'Copied' : 'Copy'}
            </button>
            <a className="btn-ghost border border-ink/10" download="spectra-recovery-codes.txt" href={`data:text/plain;charset=utf-8,${encodeURIComponent(codesText)}`}>
              <Download className="h-3.5 w-3.5" /> Download
            </a>
            <button type="button" className="btn-primary px-3 py-1.5 text-xs" onClick={() => setCodes(null)}>
              I&apos;ve saved them
            </button>
          </div>
        </div>
      )}

      {status && !codes && status.totp === 'on' && (
        <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
          <p className="text-[12.5px] text-ink/70">
            <span className="font-semibold text-profit">On</span>
            {status.totpSince ? ` since ${new Date(status.totpSince).toLocaleDateString()}` : ''} · sign-in asks for a code from your app ·{' '}
            <span className={status.recoveryCodesLeft <= 2 ? 'font-semibold text-caution' : ''}>{status.recoveryCodesLeft} recovery codes left</span>
          </p>
          <div className="flex gap-2">
            <button type="button" className="btn-ghost border border-ink/10" onClick={regenerate} disabled={busy !== null}>
              {busy === 'regen' ? <Spinner className="h-3.5 w-3.5" /> : <KeyRound className="h-3.5 w-3.5" />} New recovery codes
            </button>
            <button type="button" className="btn-ghost border border-ink/10 hover:text-loss" onClick={remove} disabled={busy !== null}>
              {busy === 'remove' ? <Spinner className="h-3.5 w-3.5" /> : null} Remove
            </button>
          </div>
        </div>
      )}

      {status && status.totp !== 'on' && !enrol && (
        <button type="button" className="btn-primary mt-4 px-3 py-1.5 text-xs" onClick={start} disabled={busy !== null}>
          {busy === 'start' ? <Spinner className="h-3.5 w-3.5" /> : <Smartphone className="h-3.5 w-3.5" />} Set up authenticator app
        </button>
      )}

      {enrol && (
        <form onSubmit={confirm} className="mt-4 grid gap-5 sm:grid-cols-[180px_1fr]">
          {/* Server-generated SVG of our own otpauth:// URI (lib/server/qr.ts). */}
          <div className="rounded-xl bg-white p-2.5" aria-label="QR code for your authenticator app" dangerouslySetInnerHTML={{ __html: enrol.qrSvg }} />
          <div className="min-w-0 space-y-3">
            <p className="text-[12.5px] leading-5 text-ink/70">
              1. Scan the QR code with your authenticator app. Can&apos;t scan? Enter this key:
              <code className="mt-1 block break-all rounded-lg bg-ink/[0.05] px-2 py-1.5 font-mono text-[12px] tracking-wider text-ink">{enrol.secret.replace(/(.{4})/g, '$1 ').trim()}</code>
            </p>
            <label className="block">
              <span className="mb-1.5 block text-[12.5px] text-ink/70">2. Enter the 6-digit code it shows</span>
              <input
                className="input num w-40 tracking-[0.3em]"
                inputMode="numeric"
                autoComplete="one-time-code"
                maxLength={6}
                value={code}
                onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))}
                autoFocus
              />
            </label>
            <div className="flex gap-2">
              <button type="button" className="btn-ghost border border-ink/10" onClick={() => (setEnrol(null), setCode(''))}>
                Cancel
              </button>
              <button type="submit" className="btn-primary px-3 py-1.5 text-xs" disabled={code.length !== 6 || busy !== null}>
                {busy === 'confirm' ? <Spinner className="h-3.5 w-3.5" /> : null} Turn on
              </button>
            </div>
          </div>
        </form>
      )}

      {error && <p className="mt-3 text-xs text-loss">{error}</p>}
    </GlassCard>
  );
}
