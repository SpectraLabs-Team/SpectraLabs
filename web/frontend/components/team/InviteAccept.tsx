'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { ArrowRight, MailWarning, Users } from 'lucide-react';
import { NEXT_KEY } from '@/components/landing/NavLink';
import { GlassCard, Spinner } from '@/components/ui/primitives';
import { useAuth } from '@/lib/auth';

type Preview =
  | { status: 'open'; workspaceName: string; role: 'editor' | 'viewer'; email: string; invitedBy: string | null; expiresAt: number }
  | { status: 'expired' | 'used' | 'revoked' | 'missing' };

const DEAD: Record<Exclude<Preview['status'], 'open'>, string> = {
  expired: 'This invitation has expired. Ask the workspace owner to send a new one.',
  used: 'This invitation has already been used.',
  revoked: 'This invitation was cancelled. Ask the workspace owner for a new one.',
  missing: 'This invitation link isn’t valid. Check that you copied the whole link.',
};

/** Show what the invite is for; sign in (or sign up) with the invited address, then accept. */
export function InviteAccept({ token }: { token: string }) {
  const router = useRouter();
  const { user, isLoading, openAuthModal } = useAuth();
  const [invite, setInvite] = useState<Preview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    fetch(`/api/invites/${encodeURIComponent(token)}`, { cache: 'no-store' })
      .then((r) => r.json())
      .then((d) => setInvite(d.invite ?? { status: 'missing' }))
      .catch(() => setError('Could not load the invitation.'));
  }, [token]);

  const signIn = (mode: 'login' | 'signup') => {
    try {
      sessionStorage.setItem(NEXT_KEY, `/invite/${token}`); // come back here after signing in
    } catch {}
    openAuthModal(mode);
  };

  const accept = async () => {
    setBusy(true);
    setError('');
    try {
      const res = await fetch(`/api/invites/${encodeURIComponent(token)}`, { method: 'POST', credentials: 'same-origin' });
      const d = await res.json().catch(() => ({}));
      if (!res.ok || !d.ok) throw new Error(d.error || 'Could not accept the invitation.');
      router.push('/studio');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not accept the invitation.');
      setBusy(false);
    }
  };

  if (!invite && !error) return <div className="flex justify-center"><Spinner className="h-5 w-5" /></div>;

  return (
    <GlassCard className="p-6 sm:p-8">
      {invite?.status === 'open' ? (
        <>
          <p className="flex items-center gap-2 font-mono text-[11px] font-medium uppercase tracking-[0.2em] text-accent">
            <Users className="h-3.5 w-3.5" /> Workspace invitation
          </p>
          <h1 className="mt-3 text-2xl font-bold tracking-tight text-ink">Join {invite.workspaceName}</h1>
          <p className="mt-2 text-sm leading-6 text-ink/65">
            {invite.invitedBy ? `${invite.invitedBy} invited ` : 'You were invited '}
            <span className="font-medium text-ink/85">{invite.email}</span> as {invite.role === 'editor' ? 'an editor (create and run)' : 'a viewer (read only)'}.
            The invitation expires {new Date(invite.expiresAt).toLocaleDateString(undefined, { day: 'numeric', month: 'long' })}.
          </p>
          <div className="mt-6">
            {isLoading ? (
              <Spinner className="h-4 w-4" />
            ) : user ? (
              <>
                <p className="mb-3 text-[12.5px] text-ink/55">
                  Signed in as <span className="font-medium text-ink/80">{user.email}</span>.
                </p>
                <button type="button" className="btn-primary" onClick={accept} disabled={busy}>
                  {busy ? <Spinner className="h-4 w-4" /> : null} Accept and open the workspace <ArrowRight className="h-4 w-4" />
                </button>
              </>
            ) : (
              <div className="flex flex-wrap gap-2">
                <button type="button" className="btn-primary" onClick={() => signIn('login')}>
                  Log in to accept <ArrowRight className="h-4 w-4" />
                </button>
                <button type="button" className="btn-secondary" onClick={() => signIn('signup')}>
                  Create an account
                </button>
              </div>
            )}
          </div>
        </>
      ) : (
        <div className="flex items-start gap-3">
          <MailWarning className="mt-0.5 h-5 w-5 shrink-0 text-caution" />
          <div>
            <h1 className="text-lg font-semibold text-ink">Invitation unavailable</h1>
            <p className="mt-1 text-sm text-ink/60">{invite ? DEAD[invite.status] : error}</p>
          </div>
        </div>
      )}
      {error && invite?.status === 'open' && <p className="mt-4 text-[12.5px] text-loss">{error}</p>}
    </GlassCard>
  );
}
