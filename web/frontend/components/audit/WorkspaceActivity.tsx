'use client';

import { useEffect, useState } from 'react';
import { AuditLog } from './AuditLog';

type Active = { id: string; name: string; role: 'owner' | 'editor' | 'viewer' };

/** The active workspace's audit log, for its owners (entries include members' IP addresses). */
export function WorkspaceActivity() {
  const [ws, setWs] = useState<Active | null | undefined>(undefined);
  useEffect(() => {
    fetch('/api/auth/session', { cache: 'no-store', credentials: 'same-origin' })
      .then((r) => r.json())
      .then((d) => setWs(d?.workspace ?? null))
      .catch(() => setWs(null));
  }, []);

  if (ws === undefined) return <div className="h-16 animate-pulse rounded-lg bg-ink/[0.04]" aria-hidden />;
  if (!ws) return <p className="text-xs text-ink/50">Could not load the active workspace.</p>;
  if (ws.role !== 'owner') return <p className="text-xs text-ink/55">Only owners of {ws.name} can see its activity log.</p>;
  return <AuditLog endpoint={`/api/workspaces/${ws.id}/audit`} />;
}
