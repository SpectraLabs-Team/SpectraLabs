import { query } from './db';
import type { WidgetLayoutItem } from '../dashboardLayout';
import { DEFAULT_LAYOUT } from '../dashboardLayout';

export interface StoredLayoutResult {
  layout: WidgetLayoutItem[];
  isWorkspaceDefault: boolean;
}

/**
 * Retrieve the active layout for a user in a workspace.
 * First checks for a user-specific custom layout; if none exists, falls back to the
 * workspace-wide default layout. If neither exists, returns the system DEFAULT_LAYOUT.
 */
export async function getWorkspaceLayout(
  workspaceId: string,
  userId?: string | null
): Promise<StoredLayoutResult> {
  try {
    if (userId) {
      const userRows = await query<{ layout: WidgetLayoutItem[] }>(
        `SELECT layout FROM workspace_layouts WHERE workspace_id = $1 AND user_id = $2 LIMIT 1`,
        [workspaceId, userId]
      );
      if (userRows.length > 0 && Array.isArray(userRows[0].layout)) {
        return { layout: userRows[0].layout, isWorkspaceDefault: false };
      }
    }

    const defaultRows = await query<{ layout: WidgetLayoutItem[] }>(
      `SELECT layout FROM workspace_layouts WHERE workspace_id = $1 AND user_id IS NULL LIMIT 1`,
      [workspaceId]
    );
    if (defaultRows.length > 0 && Array.isArray(defaultRows[0].layout)) {
      return { layout: defaultRows[0].layout, isWorkspaceDefault: true };
    }
  } catch (err) {
    // If table doesn't exist or database is unreachable, log and fallback cleanly
    console.warn('[dashboardLayoutStore] Could not read layout from database:', err);
  }

  return { layout: [...DEFAULT_LAYOUT], isWorkspaceDefault: true };
}

/**
 * Save a custom layout for a specific user in a workspace.
 */
export async function saveUserLayout(
  workspaceId: string,
  userId: string,
  layout: WidgetLayoutItem[]
): Promise<void> {
  await query(
    `INSERT INTO workspace_layouts (workspace_id, user_id, layout, updated_at)
     VALUES ($1, $2, $3::jsonb, now())
     ON CONFLICT (workspace_id, user_id) WHERE user_id IS NOT NULL
     DO UPDATE SET layout = EXCLUDED.layout, updated_at = now()`,
    [workspaceId, userId, JSON.stringify(layout)]
  );
}

/**
 * Publish a workspace-wide default layout.
 * Requires Editor or Owner role. All workspace members without custom layouts
 * will see this layout across all browsers by default.
 */
export async function saveWorkspaceDefaultLayout(
  workspaceId: string,
  layout: WidgetLayoutItem[]
): Promise<void> {
  await query(
    `INSERT INTO workspace_layouts (workspace_id, user_id, layout, updated_at)
     VALUES ($1, NULL, $2::jsonb, now())
     ON CONFLICT (workspace_id) WHERE user_id IS NULL
     DO UPDATE SET layout = EXCLUDED.layout, updated_at = now()`,
    [workspaceId, JSON.stringify(layout)]
  );
}
