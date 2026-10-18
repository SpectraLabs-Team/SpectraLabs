'use client';

import { AnimatePresence, motion } from 'framer-motion';
import { ArrowRight, Eye, EyeOff, KeyRound, Loader2, Lock, LogIn, Mail, Pencil, UserPlus, X } from 'lucide-react';
import { useRouter } from 'next/navigation';
import { useCallback, useEffect, useRef, useState, type ClipboardEvent, type FormEvent, type KeyboardEvent, type ReactNode } from 'react';
import { checkPasswordRules, EMAIL_RE, isPasswordValid, MIN_PASSWORD, roleForEmail, useAuth, type AuthMode, type Role } from '@/lib/auth';
import { cn } from '@/lib/utils';

const LEN = 6;
const RESEND_SECONDS = 60;
const empty = () => Array<string>(LEN).fill('');

// ── Placeholder transport until the Gmail SMTP / auth endpoints are wired up ──
// Swap each for a fetch to the backend. Nothing here stores or checks a password.
async function sendSignupCode(_email: string, _password: string) {
  await new Promise((r) => setTimeout(r, 900)); // POST /api/auth/signup → emails OTP
}
async function resendCode(_email: string) {
  await new Promise((r) => setTimeout(r, 900)); // POST /api/auth/resend
}
async function verifySignupCode(_email: string, _code: string) {
  await new Promise((r) => setTimeout(r, 700)); // POST /api/auth/verify → session
  return true;
}
async function loginWithPassword(_email: string, password: string) {
  await new Promise((r) => setTimeout(r, 800)); // POST /api/auth/login → session
  return password.length >= MIN_PASSWORD;
}

export function RoleBadge({ role, className }: { role: Role; className?: string }) {
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

const inputBase =
  'w-full rounded-xl border bg-white py-2.5 pl-9 pr-3 text-sm text-slate-900 outline-none transition placeholder:text-slate-400 focus:ring-2 dark:bg-canvas/70 dark:text-slate-100';
const inputOk = 'border-slate-300 focus:border-cyan-500 focus:ring-cyan-500/25 dark:border-white/10';
const inputBad = 'border-rose-400 focus:ring-rose-500/30';
const primaryBtn =
  'inline-flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-[#00F2FE] via-[#38BDF8] to-[#0284C7] py-3 text-sm font-bold text-[#03131A] shadow-glow transition hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50 disabled:shadow-none';

function Field({
  id, label, icon, error, children,
}: { id: string; label: string; icon: ReactNode; error?: string | false; children: ReactNode }) {
  return (
    <div className="mt-4">
      <label htmlFor={id} className="block text-[12px] font-medium text-ink/70">{label}</label>
      <div className="relative mt-1.5">
        <span className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-ink/40">{icon}</span>
        {children}
      </div>
      {error && <p className="mt-1.5 text-[12px] text-rose-500">{error}</p>}
    </div>
  );
}

function PasswordInput({
  id, value, onChange, onBlur, invalid, autoComplete, placeholder,
}: { id: string; value: string; onChange: (v: string) => void; onBlur: () => void; invalid: boolean; autoComplete: string; placeholder: string }) {
  const [show, setShow] = useState(false);
  return (
    <>
      <input
        id={id}
        type={show ? 'text' : 'password'}
        autoComplete={autoComplete}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onBlur={onBlur}
        placeholder={placeholder}
        aria-invalid={invalid}
        className={cn(inputBase, 'pr-10', invalid ? inputBad : inputOk)}
      />
      <button
        type="button"
        onClick={() => setShow((s) => !s)}
        aria-label={show ? 'Hide password' : 'Show password'}
        className="absolute right-2 top-1/2 -translate-y-1/2 rounded-md p-1.5 text-ink/40 transition hover:text-ink/80"
      >
        {show ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
      </button>
    </>
  );
}

export function AuthModal() {
  const router = useRouter();
  const { isAuthModalOpen, closeAuthModal, login, authMode, setAuthMode } = useAuth();
  const [step, setStep] = useState<1 | 2>(1);
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [touched, setTouched] = useState<Record<string, boolean>>({});
  const [busy, setBusy] = useState(false);
  const [verifying, setVerifying] = useState(false);
  const [digits, setDigits] = useState<string[]>(empty);
  const [error, setError] = useState<string | null>(null);
  const [left, setLeft] = useState(0);
  const boxes = useRef<(HTMLInputElement | null)[]>([]);
  const emailRef = useRef<HTMLInputElement>(null);

  const signup = authMode === 'signup';
  const emailOk = EMAIL_RE.test(email.trim());
  const passRules = checkPasswordRules(password);
  const passOk = signup ? isPasswordValid(password) : password.length >= MIN_PASSWORD;
  const confirmOk = confirm === password && confirm.length > 0;
  const role = roleForEmail(email);
  const formOk = emailOk && passOk && (!signup || confirmOk);
  const touch = (k: string) => setTouched((t) => ({ ...t, [k]: true }));

  const focusEmail = () => setTimeout(() => emailRef.current?.focus(), 50);

  // Fresh state whenever the modal opens; keep the typed email when switching modes.
  useEffect(() => {
    if (!isAuthModalOpen) return;
    setStep(1);
    setPassword('');
    setConfirm('');
    setDigits(empty());
    setError(null);
    setTouched({});
    focusEmail();
  }, [isAuthModalOpen, authMode]);

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

  const finish = useCallback(
    (r: Role) => {
      login(email, r);
      closeAuthModal();
      router.push(r === 'admin' ? '/admin' : '/studio');
    },
    [email, login, closeAuthModal, router],
  );

  const switchMode = (m: AuthMode) => setAuthMode(m);

  // ── Log In ──
  const submitLogin = async (e: FormEvent) => {
    e.preventDefault();
    setTouched({ email: true, password: true });
    if (!formOk || busy) return;
    setBusy(true);
    setError(null);
    try {
      const ok = await loginWithPassword(email.trim(), password);
      if (!ok) {
        setError('Incorrect email or password.');
        return;
      }
      finish(roleForEmail(email));
    } catch {
      setError('Could not log in. Try again.');
    } finally {
      setBusy(false);
    }
  };

  // ── Sign Up, step 1 ──
  const submitSignup = async (e: FormEvent) => {
    e.preventDefault();
    setTouched({ email: true, password: true, confirm: true });
    if (!formOk || busy) return;
    setBusy(true);
    setError(null);
    try {
      await sendSignupCode(email.trim(), password);
      setStep(2);
      setDigits(empty());
      setLeft(RESEND_SECONDS);
      setTimeout(() => boxes.current[0]?.focus(), 50);
    } catch {
      setError('Could not send the code. Try again.');
    } finally {
      setBusy(false);
    }
  };

  const resend = async () => {
    if (left > 0 || busy) return;
    setBusy(true);
    setError(null);
    try {
      await resendCode(email.trim());
      setDigits(empty());
      setLeft(RESEND_SECONDS);
      boxes.current[0]?.focus();
    } catch {
      setError('Could not resend the code.');
    } finally {
      setBusy(false);
    }
  };

  // ── Sign Up, step 2 ──
  const verify = useCallback(
    async (code: string) => {
      if (code.length !== LEN || verifying) return;
      setVerifying(true);
      setError(null);
      try {
        const ok = await verifySignupCode(email.trim(), code);
        if (!ok) {
          setError('That code is incorrect or expired.');
          setDigits(empty());
          boxes.current[0]?.focus();
          return;
        }
        finish(roleForEmail(email));
      } finally {
        setVerifying(false);
      }
    },
    [email, verifying, finish],
  );

  const fill = (text: string, start = 0) => {
    const chars = text.replace(/\D/g, '').slice(0, LEN - start).split('');
    if (!chars.length) return;
    const next = [...digits];
    chars.forEach((c, k) => (next[start + k] = c));
    setDigits(next);
    boxes.current[Math.min(start + chars.length, LEN - 1)]?.focus();
    if (next.every(Boolean)) verify(next.join(''));
  };

  const setDigit = (i: number, raw: string) => {
    const only = raw.replace(/\D/g, '');
    if (only.length > 1) return fill(only, i);
    const next = [...digits];
    next[i] = only;
    setDigits(next);
    if (only && i < LEN - 1) boxes.current[i + 1]?.focus();
    if (only && next.every(Boolean)) verify(next.join(''));
  };

  const onKeyDown = (i: number, e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Backspace') {
      e.preventDefault();
      const next = [...digits];
      if (next[i]) next[i] = '';
      else if (i > 0) {
        next[i - 1] = '';
        boxes.current[i - 1]?.focus();
      }
      setDigits(next);
    } else if (e.key === 'ArrowLeft' && i > 0) boxes.current[i - 1]?.focus();
    else if (e.key === 'ArrowRight' && i < LEN - 1) boxes.current[i + 1]?.focus();
    else if (e.key === 'Enter') verify(digits.join(''));
  };

  const onPaste = (i: number, e: ClipboardEvent<HTMLInputElement>) => {
    e.preventDefault();
    const text = e.clipboardData.getData('text');
    fill(text, text.replace(/\D/g, '').length >= LEN ? 0 : i);
  };

  const code = digits.join('');
  const view = signup ? (step === 1 ? 'signup' : 'otp') : 'login';
  const icon = view === 'login' ? <LogIn className="h-5 w-5" /> : view === 'signup' ? <UserPlus className="h-5 w-5" /> : <KeyRound className="h-5 w-5" />;

  const emailField = (
    <Field id="auth-email" label="Email" icon={<Mail className="h-4 w-4" />} error={touched.email && email.length > 0 && !emailOk && 'Enter a valid email address.'}>
      <input
        ref={emailRef}
        id="auth-email"
        type="email"
        autoComplete="email"
        inputMode="email"
        value={email}
        onChange={(e) => setEmail(e.target.value)}
        onBlur={() => touch('email')}
        placeholder="you@fund.com"
        aria-invalid={touched.email && !emailOk}
        className={cn(inputBase, touched.email && email && !emailOk ? inputBad : inputOk)}
      />
    </Field>
  );

  const switcher = (text: string, label: string, to: AuthMode) => (
    <p className="mt-5 text-center text-[13px] text-ink/60">
      {text}{' '}
      <button type="button" onClick={() => switchMode(to)} className="font-semibold text-cyan-600 hover:underline dark:text-cyan-300">
        {label}
      </button>
    </p>
  );

  return (
    <AnimatePresence>
      {isAuthModalOpen && (
        <motion.div className="fixed inset-0 z-[100] flex items-center justify-center overflow-y-auto p-4" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.18 }}>
          <div className="fixed inset-0 bg-slate-950/60 backdrop-blur-sm" onClick={closeAuthModal} aria-hidden />
          <motion.div
            role="dialog"
            aria-modal="true"
            aria-labelledby="auth-title"
            initial={{ opacity: 0, y: 16, scale: 0.97 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 10, scale: 0.98 }}
            transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
            className="relative my-auto w-full max-w-[420px] rounded-2xl border border-slate-200 bg-white/95 p-6 shadow-2xl backdrop-blur-xl dark:border-white/10 dark:bg-panel/90 sm:p-7"
          >
            <button
              type="button"
              onClick={closeAuthModal}
              aria-label="Close"
              className="absolute right-4 top-4 rounded-lg p-1.5 text-slate-500 transition hover:bg-slate-100 hover:text-slate-900 dark:text-slate-400 dark:hover:bg-white/5 dark:hover:text-white"
            >
              <X className="h-4 w-4" />
            </button>

            <div className="flex h-11 w-11 items-center justify-center rounded-xl border border-cyan-500/30 bg-cyan-500/10 text-cyan-600 dark:text-cyan-300">{icon}</div>

            <AnimatePresence mode="wait" initial={false}>
              {view === 'login' && (
                <motion.form key="login" onSubmit={submitLogin} noValidate initial={{ opacity: 0, x: -12 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: 12 }} transition={{ duration: 0.16 }}>
                  <h2 id="auth-title" className="mt-4 text-xl font-bold tracking-tight text-ink">Welcome Back</h2>
                  <p className="mt-1.5 text-sm text-ink/60">Log in to Spectra Labs.</p>
                  {emailField}
                  <Field id="auth-password" label="Password" icon={<Lock className="h-4 w-4" />} error={touched.password && !passOk && `At least ${MIN_PASSWORD} characters.`}>
                    <PasswordInput id="auth-password" value={password} onChange={setPassword} onBlur={() => touch('password')} invalid={!!touched.password && !passOk} autoComplete="current-password" placeholder="••••••••" />
                  </Field>
                  {error && <p className="mt-3 text-[12px] text-rose-500">{error}</p>}
                  <button type="submit" disabled={busy} className={cn(primaryBtn, 'mt-6')}>
                    {busy ? <><Loader2 className="h-4 w-4 animate-spin" /> Logging in…</> : <>Log In <ArrowRight className="h-4 w-4" /></>}
                  </button>
                  {switcher("Don't have an account?", 'Sign Up', 'signup')}
                </motion.form>
              )}

              {view === 'signup' && (
                <motion.form key="signup" onSubmit={submitSignup} noValidate initial={{ opacity: 0, x: -12 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: -12 }} transition={{ duration: 0.16 }}>
                  <div className="mt-4 flex items-start justify-between gap-3">
                    <h2 id="auth-title" className="text-xl font-bold tracking-tight text-ink">Create your Account</h2>
                    {emailOk && <RoleBadge role={role} className="mt-1 shrink-0" />}
                  </div>
                  <p className="mt-1.5 text-sm text-ink/60">We&apos;ll email a 6-digit code to verify it&apos;s you.</p>
                  {emailField}
                  <Field id="auth-new-password" label="Password" icon={<Lock className="h-4 w-4" />} error={touched.password && !passOk && 'Password does not meet all requirements below.'}>
                    <PasswordInput id="auth-new-password" value={password} onChange={setPassword} onBlur={() => touch('password')} invalid={!!touched.password && !passOk} autoComplete="new-password" placeholder="Min. 8 characters" />
                    <div className="mt-2.5 grid grid-cols-2 gap-1.5 rounded-lg border border-ink/[0.06] bg-ink/[0.02] p-2.5 text-[11px]">
                      <span className={cn('flex items-center gap-1.5 transition-colors', passRules.length ? 'text-emerald-600 dark:text-emerald-400 font-medium' : 'text-ink/40')}>
                        <span className={cn('h-1.5 w-1.5 rounded-full', passRules.length ? 'bg-emerald-500 shadow-[0_0_6px_#10B981]' : 'bg-ink/30')} />
                        8+ characters
                      </span>
                      <span className={cn('flex items-center gap-1.5 transition-colors', passRules.letter ? 'text-emerald-600 dark:text-emerald-400 font-medium' : 'text-ink/40')}>
                        <span className={cn('h-1.5 w-1.5 rounded-full', passRules.letter ? 'bg-emerald-500 shadow-[0_0_6px_#10B981]' : 'bg-ink/30')} />
                        1 letter (a-z)
                      </span>
                      <span className={cn('flex items-center gap-1.5 transition-colors', passRules.number ? 'text-emerald-600 dark:text-emerald-400 font-medium' : 'text-ink/40')}>
                        <span className={cn('h-1.5 w-1.5 rounded-full', passRules.number ? 'bg-emerald-500 shadow-[0_0_6px_#10B981]' : 'bg-ink/30')} />
                        1 number (0-9)
                      </span>
                      <span className={cn('flex items-center gap-1.5 transition-colors', passRules.special ? 'text-emerald-600 dark:text-emerald-400 font-medium' : 'text-ink/40')}>
                        <span className={cn('h-1.5 w-1.5 rounded-full', passRules.special ? 'bg-emerald-500 shadow-[0_0_6px_#10B981]' : 'bg-ink/30')} />
                        1 special char (!@#$)
                      </span>
                    </div>
                  </Field>
                  <Field id="auth-confirm" label="Confirm Password" icon={<Lock className="h-4 w-4" />} error={touched.confirm && !confirmOk && 'Passwords don’t match.'}>
                    <PasswordInput id="auth-confirm" value={confirm} onChange={setConfirm} onBlur={() => touch('confirm')} invalid={!!touched.confirm && !confirmOk} autoComplete="new-password" placeholder="Repeat password" />
                  </Field>
                  {error && <p className="mt-3 text-[12px] text-rose-500">{error}</p>}
                  <button type="submit" disabled={busy} className={cn(primaryBtn, 'mt-6')}>
                    {busy ? <><Loader2 className="h-4 w-4 animate-spin" /> Sending code…</> : <>Send Verification Code <ArrowRight className="h-4 w-4" /></>}
                  </button>
                  {switcher('Already have an account?', 'Log In', 'login')}
                </motion.form>
              )}

              {view === 'otp' && (
                <motion.div key="otp" initial={{ opacity: 0, x: 12 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: 12 }} transition={{ duration: 0.16 }}>
                  <h2 id="auth-title" className="mt-4 text-xl font-bold tracking-tight text-ink">Verify your email</h2>
                  <p className="mt-1.5 flex flex-wrap items-center gap-x-1.5 text-sm text-ink/60">
                    Code sent to <span className="font-medium text-ink/90">{email.trim()}</span>
                    <button
                      type="button"
                      onClick={() => {
                        setStep(1);
                        setError(null);
                        focusEmail();
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

                  <button type="button" onClick={() => verify(code)} disabled={code.length !== LEN || verifying} className={cn(primaryBtn, 'mt-6')}>
                    {verifying ? <><Loader2 className="h-4 w-4 animate-spin" /> Verifying…</> : <>Verify &amp; Complete Registration <ArrowRight className="h-4 w-4" /></>}
                  </button>

                  <p className="mt-4 text-center text-[12.5px] text-ink/55">
                    Didn&apos;t get it?{' '}
                    {left > 0 ? (
                      <span className="font-mono tabular-nums">Resend in 0:{String(left).padStart(2, '0')}</span>
                    ) : (
                      <button type="button" onClick={resend} disabled={busy} className="font-semibold text-cyan-600 hover:underline disabled:opacity-50 dark:text-cyan-300">
                        {busy ? 'Sending…' : 'Resend Code'}
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
