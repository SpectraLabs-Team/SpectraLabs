import type { Auth } from './sessions';
import { getMembership, ROLE_RANK, type Workspace, type WorkspaceRole } from './workspaces';

/**
 * Authorization (M19 task 4). Every route that reads or writes workspace data goes through
 * requireRole (route helper: requireWorkspace in ./requireUser); platform-admin routes go through
 * requireAdmin (./requireUser). This file has no Next.js imports so it can be unit-tested.
 *
 *   viewer  read only
 *   editor  + create and run (datasets, runs, jobs, engine computations)
 *   owner   + members, settings, delete
 *
 * Not a member → 404 (the same answer as "doesn't exist", so ids of other teams can't be probed).
 * Member with too low a role → 403.
 */

export class AuthzError extends Error {
  constructor(public status: 401 | 403 | 404, message: string) {
    super(message);
    this.name = 'AuthzError';
  }
}

export const hasRole = (role: WorkspaceRole, minRole: WorkspaceRole) => ROLE_RANK[role] >= ROLE_RANK[minRole];

const ACTION: Record<WorkspaceRole, string> = {
  viewer: 'view this workspace',
  editor: 'create or run things in this workspace (editor role needed)',
  owner: 'manage this workspace (owner role needed)',
};

/**
 * The workspace (with the caller's role in it) if the signed-in user has at least `minRole` there;
 * throws AuthzError otherwise. Membership is read from the database, never from the client.
 */
export async function requireRole(auth: Auth | null, workspaceId: string, minRole: WorkspaceRole): Promise<Workspace> {
  if (!auth) throw new AuthzError(401, 'Not signed in.');
  const ws = workspaceId === auth.workspace.id ? auth.workspace : await getMembership(auth.user.id, workspaceId);
  if (!ws) throw new AuthzError(404, 'Workspace not found.');
  if (!hasRole(ws.role, minRole)) throw new AuthzError(403, `You can't ${ACTION[minRole]}.`);
  return ws;
}
