'use client';

import { AnimatePresence, motion } from 'framer-motion';
import { ArrowRight, KeyRound, Loader2, Mail, Pencil, ShieldCheck, X } from 'lucide-react';
import { useCallback, useEffect, useRef, useState, type ClipboardEvent, type FormEvent, type KeyboardEvent } from 'react';
import { EMAIL_RE, roleForEmail, useAuth } from '@/lib/auth';
import { cn } from '@/lib/utils';

const LEN = 6;
const RESEND_SECONDS = 60;
const empty = () => Array<string>(LEN).fill('');

// Placeholder transport until the Gmail SMTP endpoints are wired up.
// Swap these for fetch('/api/auth/send-otp') / fetch('/api/auth/verify-otp').
async function sendCode(_email: string) {
  await new Promise((r) => setTimeout(r, 900));
}
async function verifyCode(_email: string, _code: string) {
  await new Promise((r) => setTimeout(r, 700));
  return true;
}

export function RoleBadge({ role, className }: { role: 'admin' | 'user'; className?: string }) {
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[11px] font-semibold',
        role === 'admin'
          ? 'border-cyan-500/40 bg-cyan-500/10 text-cyan-700 dark:text-cyan-300'
          : 'border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300',
        className,
      )}
    >
      <span className={cn('h-1.5 w-1.5 rounded-full', role === 'admin' ? 'bg-cyan-500' : 'bg-emerald-500')} />
      {role === 'admin' ? 'Admin Access' : 'User Access'}
    </span>
  );
}

export function AuthModal() {
  const { isAuthModalOpen, closeAuthModal, login } = useAuth();
  const [step, setStep] = useState<1 | 2>(1);
  const [email, setEmail] = useState('');
  const [touched, setTouched] = useState(false);
  const [sending, setSending] = useState(false);
  const [verifying, setVerifying] = useState(false);
  const [digits, setDigits] = useState<string[]>(empty);
  const [error, setError] = useState<string | null>(null);
  const [left, setLeft] = useState(0);
  const boxes = useRef<(HTMLInputElement | null)[]>([]);
  const emailRef = useRef<HTMLInputElement>(null);

  const valid = EMAIL_RE.test(email.trim());
  const role = roleForEmail(email);

  // Reset whenever the modal opens.
  useEffect(() => {
    if (!isAuthModalOpen) return;
    setStep(1);
    setDigits(empty());
    setError(null);
    setTouched(false);
    setTimeout(() => emailRef.current?.focus(), 50);
  }, [isAuthModalOpen]);

  // Escape closes; lock page scroll while open.
  useEffect(() => {
    if (!isAuthModalOpen) return;
    const onKey = (e: globalThis.KeyboardEvent) => e.key === 'Escape' && closeAuthModal();
    window.addEventListener('keydown', onKey);
    const prev = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      window.removeEventListener('keydown', onKey);
      document.body.style.overflow = prev;
    };
  }, [isAuthModalOpen, closeAuthModal]);

  useEffect(() => {
    if (left <= 0) return;
    const t = setTimeout(() => setLeft((s) => s - 1), 1000);
    return () => clearTimeout(t);
  }, [left]);

  const submitEmail = async (e?: FormEvent) => {
    e?.preventDefault();
    setTouched(true);
    if (!valid || sending) return;
    setSending(true);
    setError(null);
    try {
      await sendCode(email.trim());
      setStep(2);
      setDigits(empty());
      setLeft(RESEND_SECONDS);
      setTimeout(() => boxes.current[0]?.focus(), 50);
    } catch {
      setError('Could not send the code. Try again.');
    } finally {
      setSending(false);
    }
  };

  const resend = async () => {
    if (left > 0 || sending) return;
    setSending(true);
    setError(null);
    try {
      await sendCode(email.trim());
      setDigits(empty());
      setLeft(RESEND_SECONDS);
      boxes.current[0]?.focus();
    } catch {
      setError('Could not resend the code.');
    } finally {
      setSending(false);
    }
  };

  const verify = useCallback(
    async (code: string) => {
      if (code.length !== LEN || verifying) return;
      setVerifying(true);
      setError(null);
      try {
        const ok = await verifyCode(email.trim(), code);
        if (!ok) {
          setError('That code is incorrect or expired.');
          setDigits(empty());
          boxes.current[0]?.focus();
          return;
        }
        login(email, roleForEmail(email));
        closeAuthModal();
      } finally {
        setVerifying(false);
      }
    },
    [email, verifying, login, closeAuthModal],
  );

  const setDigit = (i: number, raw: string) => {
    const only = raw.replace(/\D/g, '');
    if (only.length > 1) {
      fill(only, i);
      return;
    }
    const next = [...digits];
    next[i] = only;
    setDigits(next);
    if (only && i < LEN - 1) boxes.current[i + 1]?.focus();
    if (only && next.every(Boolean)) verify(next.join(''));
  };

  const fill = (text: string, start = 0) => {
    const chars = text.replace(/\D/g, '').slice(0, LEN - start).split('');
    if (!chars.length) return;
    const next = [...digits];
    chars.forEach((c, k) => (next[start + k] = c));
    setDigits(next);
    const last = Math.min(start + chars.length, LEN - 1);
    boxes.current[last]?.focus();
    if (next.every(Boolean)) verify(next.join(''));
  };

  const onKeyDown = (i: number, e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Backspace') {
      e.preventDefault();
      const next = [...digits];
      if (next[i]) {
        next[i] = '';
      } else if (i > 0) {
        next[i - 1] = '';
        boxes.current[i - 1]?.focus();
      }
      setDigits(next);
    } else if (e.key === 'ArrowLeft' && i > 0) {
      boxes.current[i - 1]?.focus();
    } else if (e.key === 'ArrowRight' && i < LEN - 1) {
      boxes.current[i + 1]?.focus();
    } else if (e.key === 'Enter') {
      verify(digits.join(''));
    }
  };

  const onPaste = (i: number, e: ClipboardEvent<HTMLInputElement>) => {
    e.preventDefault();
    const text = e.clipboardData.getData('text');
    fill(text, text.replace(/\D/g, '').length >= LEN ? 0 : i);
  };

  const code = digits.join('');

  return (
    <AnimatePresence>
      {isAuthModalOpen && (
        <motion.div
          className="fixed inset-0 z-[100] flex items-center justify-center p-4"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.18 }}
        >
          <div className="absolute inset-0 bg-slate-950/60 backdrop-blur-sm" onClick={closeAuthModal} aria-hidden />
          <motion.div
            role="dialog"
            aria-modal="true"
            aria-labelledby="auth-title"
            initial={{ opacity: 0, y: 16, scale: 0.97 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 10, scale: 0.98 }}
            transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
            className="relative w-full max-w-[420px] rounded-2xl border border-slate-200 bg-white/95 p-6 shadow-2xl backdrop-blur-xl dark:border-white/10 dark:bg-panel/90 sm:p-7"
          >
            <button
              type="button"
              onClick={closeAuthModal}
              aria-label="Close"
              className="absolute right-4 top-4 rounded-lg p-1.5 text-slate-500 transition hover:bg-slate-100 hover:text-slate-900 dark:text-slate-400 dark:hover:bg-white/5 dark:hover:text-white"
            >
              <X className="h-4 w-4" />
            </button>

            <div className="flex h-11 w-11 items-center justify-center rounded-xl border border-cyan-500/30 bg-cyan-500/10 text-cyan-600 dark:text-cyan-300">
              {step === 1 ? <ShieldCheck className="h-5 w-5" /> : <KeyRound className="h-5 w-5" />}
            </div>

            <AnimatePresence mode="wait" initial={false}>
              {step === 1 ? (
                <motion.form
                  key="s1"
                  onSubmit={submitEmail}
                  initial={{ opacity: 0, x: -12 }}
                  animate={{ opacity: 1, x: 0 }}
                  exit={{ opacity: 0, x: -12 }}
                  transition={{ duration: 0.16 }}
                  noValidate
                >
                  <h2 id="auth-title" className="mt-4 text-xl font-bold tracking-tight text-ink">Sign in to Spectra Labs</h2>
                  <p className="mt-1.5 text-sm text-ink/60">We&apos;ll email you a 6-digit verification code.</p>

                  <label htmlFor="auth-email" className="mt-6 block text-[12px] font-medium text-ink/70">Work email</label>
                  <div className="relative mt-1.5">
                    <Mail className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-ink/40" />
                    <input
                      ref={emailRef}
                      id="auth-email"
                      type="email"
                      autoComplete="email"
                      inputMode="email"
                      value={email}
                      onChange={(e) => setEmail(e.target.value)}
                      onBlur={() => setTouched(true)}
                      placeholder="you@fund.com"
                      aria-invalid={touched && !valid}
                      className={cn(
                        'w-full rounded-xl border bg-white py-2.5 pl-9 pr-3 text-sm text-slate-900 outline-none transition placeholder:text-slate-400 focus:ring-2 dark:bg-canvas/70 dark:text-slate-100',
                        touched && !valid && email
                          ? 'border-rose-400 focus:ring-rose-500/30'
                          : 'border-slate-300 focus:border-cyan-500 focus:ring-cyan-500/25 dark:border-white/10',
                      )}
                    />
                  </div>
                  <div className="mt-2 flex min-h-[22px] items-center justify-between gap-2">
                    {touched && email && !valid ? (
                      <p className="text-[12px] text-rose-500">Enter a valid email address.</p>
                    ) : valid ? (
                      <RoleBadge role={role} />
                    ) : (
                      <span />
                    )}
                  </div>

                  {error && <p className="mt-2 text-[12px] text-rose-500">{error}</p>}

                  <button
                    type="submit"
                    disabled={!valid || sending}
                    className="mt-5 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-cyan-500 to-sky-500 py-3 text-sm font-bold text-white shadow-[0_0_20px_rgba(6,182,212,0.35)] transition hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50 disabled:shadow-none"
                  >
                    {sending ? (
                      <>
                        <Loader2 className="h-4 w-4 animate-spin" /> Sending code…
                      </>
                    ) : (
                      <>
                        Send Verification Code <ArrowRight className="h-4 w-4" />
                      </>
                    )}
                  </button>
                </motion.form>
              ) : (
                <motion.div
                  key="s2"
                  initial={{ opacity: 0, x: 12 }}
                  animate={{ opacity: 1, x: 0 }}
                  exit={{ opacity: 0, x: 12 }}
                  transition={{ duration: 0.16 }}
                >
                  <h2 id="auth-title" className="mt-4 text-xl font-bold tracking-tight text-ink">Enter verification code</h2>
                  <p className="mt-1.5 flex flex-wrap items-center gap-x-1.5 text-sm text-ink/60">
                    Sent to <span className="font-medium text-ink/90">{email.trim()}</span>
                    <button
                      type="button"
                      onClick={() => {
                        setStep(1);
                        setError(null);
                        setTimeout(() => emailRef.current?.focus(), 50);
                      }}
                      className="inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[12px] font-semibold text-cyan-600 transition hover:bg-cyan-500/10 dark:text-cyan-300"
                    >
                      <Pencil className="h-3 w-3" /> Edit
                    </button>
                  </p>

                  <div className="mt-6 flex justify-between gap-2" role="group" aria-label="6-digit code">
                    {digits.map((d, i) => (
                      <input
                        key={i}
                        ref={(el) => {
                          boxes.current[i] = el;
                        }}
                        value={d}
                        onChange={(e) => setDigit(i, e.target.value)}
                        onKeyDown={(e) => onKeyDown(i, e)}
                        onPaste={(e) => onPaste(i, e)}
                        onFocus={(e) => e.target.select()}
                        inputMode="numeric"
                        autoComplete={i === 0 ? 'one-time-code' : 'off'}
                        maxLength={LEN}
                        disabled={verifying}
                        aria-label={`Digit ${i + 1}`}
                        className={cn(
                          'h-12 w-full min-w-0 rounded-xl border bg-white text-center font-mono text-lg font-semibold text-slate-900 outline-none transition focus:border-cyan-500 focus:ring-2 focus:ring-cyan-500/25 disabled:opacity-60 dark:bg-canvas/70 dark:text-white sm:h-14',
                          error ? 'border-rose-400' : d ? 'border-cyan-500/50' : 'border-slate-300 dark:border-white/10',
                        )}
                      />
                    ))}
                  </div>

                  {error && <p className="mt-3 text-[12px] text-rose-500">{error}</p>}

                  <button
                    type="button"
                    onClick={() => verify(code)}
                    disabled={code.length !== LEN || verifying}
                    className="mt-6 inline-flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-cyan-500 to-sky-500 py-3 text-sm font-bold text-white shadow-[0_0_20px_rgba(6,182,212,0.35)] transition hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50 disabled:shadow-none"
                  >
                    {verifying ? (
                      <>
                        <Loader2 className="h-4 w-4 animate-spin" /> Verifying…
                      </>
                    ) : (
                      <>
                        Verify &amp; Launch <ArrowRight className="h-4 w-4" />
                      </>
                    )}
                  </button>

                  <p className="mt-4 text-center text-[12.5px] text-ink/55">
                    Didn&apos;t get it?{' '}
                    {left > 0 ? (
                      <span className="font-mono tabular-nums">Resend in 0:{String(left).padStart(2, '0')}</span>
                    ) : (
                      <button type="button" onClick={resend} disabled={sending} className="font-semibold text-cyan-600 hover:underline disabled:opacity-50 dark:text-cyan-300">
                        {sending ? 'Sending…' : 'Resend Code'}
                      </button>
                    )}
                  </p>
                </motion.div>
              )}
            </AnimatePresence>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
