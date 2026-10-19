'use client';

import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { Copy, Crown, LogOut, Mail, Trash2, UserPlus } from 'lucide-react';
import { useReauth } from '@/components/auth/useReauth';
import { Spinner } from '@/components/ui/primitives';
import { useAuth } from '@/lib/auth';
import { cn } from '@/lib/utils';

type Role = 'owner' | 'editor' | 'viewer';
type Member = { userId: string; email: string; name: string; role: Role; joinedAt: number };
type Invite = { id: string; email: string; role: 'editor' | 'viewer'; invitedBy: string | null; expiresAt: number };
type Active = { id: string; name: string; personal: boolean; role: Role };

const ROLE_HELP: Record<Role, string> = { owner: 'members, settings, delete', editor: 'create and run', viewer: 'read only' };

/**
 * Settings → Members of the active workspace (M19 task 12). Owners invite by email, change roles,
 * remove members and transfer ownership (each asks for a fresh "confirm it's you"); everyone else
 * sees the list and can leave. Personal workspaces just say so.
 */
export function WorkspaceMembers() {
  const { user } = useAuth();
  const { call, dialog } = useReauth();
  const [ws, setWs] = useState<Active | null | undefined>(undefined);
  const [members, setMembers] = useState<Member[]>([]);
  const [invites, setInvites] = useState<Invite[]>([]);
  const [email, setEmail] = useState('');
  const [role, setRole] = useState<'editor' | 'viewer'>('editor');
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [lastLink, setLastLink] = useState('');

  const load = useCallback(async () => {
    try {
      const s = await call<{ workspace: Active | null }>('/api/auth/session');
      setWs(s.workspace);
      if (!s.workspace || s.workspace.personal) return;
      const d = await call<{ members: Member[] }>(`/api/workspaces/${s.workspace.id}`);
      setMembers(d.members);
      if (s.workspace.role === 'owner') setInvites((await call<{ invites: Invite[] }>(`/api/workspaces/${s.workspace.id}/invites`)).invites);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not load members.');
      setWs((w) => w ?? null);
    }
  }, [call]);

  useEffect(() => {
    load();
  }, [load]);

  const run = async (key: string, fn: () => Promise<string | void>) => {
    setBusy(key);
    setError('');
    setNotice('');
    try {
      const msg = await fn();
      if (msg) setNotice(msg);
      await load();
    } catch (e) {
      if (!(e instanceof Error && e.message === 'Cancelled.')) setError(e instanceof Error ? e.message : 'Something went wrong.');
    } finally {
      setBusy(null);
    }
  };

  if (ws === undefined) return <div className="h-16 animate-pulse rounded-lg bg-ink/[0.04]" aria-hidden />;
  if (!ws) return <p className="text-xs text-loss">{error || 'Could not load the active workspace.'}</p>;
  if (ws.personal) {
    return <p className="text-[12.5px] text-ink/55">Your personal workspace is just for you. Create a team workspace from the switcher in the header to work with others.</p>;
  }

  const owner = ws.role === 'owner';
  const base = `/api/workspaces/${ws.id}`;

  const invite = (e: FormEvent) => {
    e.preventDefault();
    run('invite', async () => {
      const d = await call<{ invite: Invite; link: string; emailed: boolean }>(`${base}/invites`, 'POST', { email: email.trim(), role });
      setEmail('');
      setLastLink(d.link);
      return d.emailed ? `Invite emailed to ${d.invite.email}.` : `Couldn’t email ${d.invite.email}. Share the link below with them instead.`;
    });
  };

  return (
    <div className="space-y-4">
      {dialog}
      <ul className="divide-y divide-ink/[0.06] rounded-xl border border-ink/[0.08]">
        {members.map((m) => {
          const me = m.email === user?.email;
          return (
            <li key={m.userId} className="flex flex-wrap items-center gap-x-3 gap-y-2 px-3.5 py-2.5">
              <div className="min-w-0 flex-1">
                <p className="truncate text-[13px] font-medium text-ink">
                  {m.name || m.email} {me && <span className="text-[11px] font-normal text-ink/45">(you)</span>}
                </p>
                <p className="truncate text-[11.5px] text-ink/50">{m.email}</p>
              </div>
              {owner && !me ? (
                <select
                  aria-label={`Role for ${m.email}`}
                  className="rounded-lg border border-ink/10 bg-canvas/70 px-2 py-1 text-[12px] text-ink"
                  value={m.role}
                  disabled={busy !== null}
                  onChange={(e) => {
                    const to = e.target.value as Role;
                    run(`role-${m.userId}`, async () => {
                      await call(`${base}/members/${m.userId}`, 'PATCH', { role: to });
                      return `${m.email} is now ${to === 'owner' ? 'an owner' : `a${to === 'editor' ? 'n' : ''} ${to}`}.`;
                    });
                  }}
                >
                  <option value="owner">Owner</option>
                  <option value="editor">Editor</option>
                  <option value="viewer">Viewer</option>
                </select>
              ) : (
                <span className={cn('rounded-full px-2 py-0.5 text-[11px] font-semibold', m.role === 'owner' ? 'bg-accent/12 text-accent' : 'bg-ink/[0.06] text-ink/65')} title={ROLE_HELP[m.role]}>
                  {m.role[0].toUpperCase() + m.role.slice(1)}
                </span>
              )}
              {owner && !me && (
                <div className="flex gap-1">
                  {m.role !== 'owner' && (
                    <button
                      type="button"
                      className="btn-ghost border border-ink/10"
                      title="Make owner; you become an editor"
                      disabled={busy !== null}
                      onClick={() =>
                        window.confirm(`Make ${m.email} the owner? You'll become an editor.`) &&
                        run(`transfer-${m.userId}`, async () => {
                          await call(`${base}/transfer`, 'POST', { userId: m.userId });
                          setTimeout(() => window.location.reload(), 600); // your own role changed
                          return `${m.email} now owns ${ws.name}.`;
                        })
                      }
                    >
                      <Crown className="h-3.5 w-3.5" />
                    </button>
                  )}
                  <button
                    type="button"
                    className="btn-ghost border border-ink/10 hover:text-loss"
                    aria-label={`Remove ${m.email}`}
                    disabled={busy !== null}
                    onClick={() =>
                      window.confirm(`Remove ${m.email} from ${ws.name}?`) &&
                      run(`remove-${m.userId}`, async () => {
                        await call(`${base}/members/${m.userId}`, 'DELETE');
                        return `${m.email} was removed.`;
                      })
                    }
                  >
                    {busy === `remove-${m.userId}` ? <Spinner className="h-3.5 w-3.5" /> : <Trash2 className="h-3.5 w-3.5" />}
                  </button>
                </div>
              )}
              {me && (
                <button
                  type="button"
                  className="btn-ghost border border-ink/10"
                  disabled={busy !== null}
                  onClick={() =>
                    window.confirm(`Leave ${ws.name}? You'll need a new invite to come back.`) &&
                    run('leave', async () => {
                      await call(`${base}/members/${m.userId}`, 'DELETE');
                      setTimeout(() => window.location.reload(), 400);
                      return `You left ${ws.name}.`;
                    })
                  }
                >
                  <LogOut className="h-3.5 w-3.5" /> Leave
                </button>
              )}
            </li>
          );
        })}
      </ul>

      {owner && (
        <>
          <form onSubmit={invite} className="flex flex-wrap items-end gap-2">
            <label className="min-w-[220px] flex-1">
              <span className="mb-1 block text-[11.5px] font-medium text-ink/60">Invite by email</span>
              <input className="input py-1.5 text-[13px]" type="email" placeholder="name@fund.com" value={email} onChange={(e) => setEmail(e.target.value)} autoComplete="off" />
            </label>
            <select aria-label="Role" className="rounded-lg border border-ink/10 bg-canvas/70 px-2 py-2 text-[12.5px] text-ink" value={role} onChange={(e) => setRole(e.target.value as 'editor' | 'viewer')}>
              <option value="editor">Editor · create and run</option>
              <option value="viewer">Viewer · read only</option>
            </select>
            <button type="submit" className="btn-primary px-3 py-2 text-xs" disabled={!email.trim() || busy !== null}>
              {busy === 'invite' ? <Spinner className="h-3.5 w-3.5" /> : <UserPlus className="h-3.5 w-3.5" />} Invite
            </button>
          </form>
          {lastLink && (
            <p className="flex flex-wrap items-center gap-2 text-[11.5px] text-ink/55">
              Invite link (works once, only for that email, 7 days):
              <code className="max-w-full truncate rounded bg-ink/[0.05] px-1.5 py-0.5 font-mono text-[11px] text-ink/75">{lastLink}</code>
              <button type="button" className="btn-ghost border border-ink/10" onClick={() => navigator.clipboard?.writeText(lastLink)}>
                <Copy className="h-3 w-3" /> Copy
              </button>
            </p>
          )}
          {invites.length > 0 && (
            <div>
              <p className="mb-1.5 text-[11.5px] font-medium text-ink/55">Pending invites</p>
              <ul className="space-y-1.5">
                {invites.map((i) => (
                  <li key={i.id} className="flex items-center gap-2 text-[12.5px]">
                    <Mail className="h-3.5 w-3.5 text-ink/40" />
                    <span className="min-w-0 flex-1 truncate text-ink/80">
                      {i.email} · {i.role} · expires {new Date(i.expiresAt).toLocaleDateString(undefined, { day: 'numeric', month: 'short' })}
                    </span>
                    <button
                      type="button"
                      className="btn-ghost border border-ink/10"
                      disabled={busy !== null}
                      onClick={() =>
                        run(`revoke-${i.id}`, async () => {
                          await call(`${base}/invites/${i.id}`, 'DELETE');
                          return `Invite to ${i.email} cancelled.`;
                        })
                      }
                    >
                      Cancel
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}

      {error && <p className="text-xs text-loss">{error}</p>}
      {notice && <p className="text-xs text-profit">{notice}</p>}
    </div>
  );
}
