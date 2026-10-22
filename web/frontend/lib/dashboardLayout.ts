export type WidgetId =
  | 'summary_kpis'
  | 'risk_gauges'
  | 'recent_runs'
  | 'kill_switches'
  | 'audit_decisions'
  | 'quick_actions';

export interface WidgetLayoutItem {
  id: WidgetId;
  title: string;
  description: string;
  visible: boolean;
}

export const DEFAULT_LAYOUT: WidgetLayoutItem[] = [
  {
    id: 'summary_kpis',
    title: 'Workspace KPIs & Capital Posture',
    description: 'Risk controls status, total backtests, pre-trade verdicts, and live capital summary.',
    visible: true,
  },
  {
    id: 'risk_gauges',
    title: 'Live Risk Limits & Breaches (M8)',
    description: 'Live gauges for gross exposure, net exposure, leverage, drawdown, and daily loss.',
    visible: true,
  },
  {
    id: 'recent_runs',
    title: 'Recent Backtest Runs',
    description: 'Recent runs with status, return, Sharpe, and M5 experiment registry links.',
    visible: true,
  },
  {
    id: 'kill_switches',
    title: 'Active Kill Switches & Halts',
    description: 'Emergency trading halt indicators and circuit breaker controls.',
    visible: true,
  },
  {
    id: 'audit_decisions',
    title: 'Latest Pre-Trade Risk Verdicts',
    description: 'Enforcement audit trail of approved, reduced, and blocked orders.',
    visible: true,
  },
  {
    id: 'quick_actions',
    title: 'Quick Actions & Studio Shortcuts',
    description: 'One-click launch pads for backtests, risk parameters, and research.',
    visible: true,
  },
];

const STORAGE_PREFIX = 'spectra_overview_layout';

export function getLayoutStorageKey(workspaceId?: string | null): string {
  return `${STORAGE_PREFIX}_${workspaceId || 'default'}`;
}

export function loadSavedLayout(workspaceId?: string | null): WidgetLayoutItem[] {
  if (typeof window === 'undefined') return DEFAULT_LAYOUT;
  try {
    const raw = localStorage.getItem(getLayoutStorageKey(workspaceId));
    if (!raw) return DEFAULT_LAYOUT;
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return DEFAULT_LAYOUT;

    const result: WidgetLayoutItem[] = [];
    const seen = new Set<string>();

    for (const item of parsed) {
      if (item && typeof item.id === 'string') {
        const def = DEFAULT_LAYOUT.find((d) => d.id === item.id);
        if (def) {
          result.push({
            id: def.id,
            title: def.title,
            description: def.description,
            visible: typeof item.visible === 'boolean' ? item.visible : def.visible,
          });
          seen.add(def.id);
        }
      }
    }

    for (const def of DEFAULT_LAYOUT) {
      if (!seen.has(def.id)) {
        result.push({ ...def });
      }
    }

    return result;
  } catch {
    return DEFAULT_LAYOUT;
  }
}

export function saveLayout(layout: WidgetLayoutItem[], workspaceId?: string | null): void {
  if (typeof window === 'undefined') return;
  try {
    localStorage.setItem(getLayoutStorageKey(workspaceId), JSON.stringify(layout));
  } catch {
    /* storage full or unavailable */
  }
}

export function resetLayout(workspaceId?: string | null): WidgetLayoutItem[] {
  if (typeof window !== 'undefined') {
    try {
      localStorage.removeItem(getLayoutStorageKey(workspaceId));
    } catch {
      /* ignore */
    }
  }
  return DEFAULT_LAYOUT.map((item) => ({ ...item }));
}

/** Encode layout to a compact URL query string for sharing */
export function encodeLayoutForSharing(layout: WidgetLayoutItem[]): string {
  return layout.map((item) => `${item.id}:${item.visible ? '1' : '0'}`).join(';');
}

/** Decode layout from a URL query string */
export function decodeLayoutFromSharing(encoded: string): WidgetLayoutItem[] | null {
  if (!encoded || typeof encoded !== 'string' || !encoded.trim()) return null;
  try {
    const parts = encoded.split(';');
    const result: WidgetLayoutItem[] = [];
    const seen = new Set<string>();

    for (const part of parts) {
      const [id, vis] = part.split(':');
      const def = DEFAULT_LAYOUT.find((d) => d.id === id);
      if (def) {
        result.push({
          id: def.id,
          title: def.title,
          description: def.description,
          visible: vis === '1',
        });
        seen.add(def.id);
      }
    }

    if (seen.size === 0) return null;

    for (const def of DEFAULT_LAYOUT) {
      if (!seen.has(def.id)) {
        result.push({ ...def });
      }
    }

    return result.length > 0 ? result : null;
  } catch {
    return null;
  }
}

/** Fetch layout from the server (PostgreSQL), falling back to null on network or server error */
export async function fetchServerLayout(
  workspaceId: string
): Promise<{ layout: WidgetLayoutItem[]; isWorkspaceDefault: boolean } | null> {
  if (!workspaceId) return null;
  try {
    const res = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/layout`, {
      credentials: 'same-origin',
      cache: 'no-store',
    });
    if (!res.ok) return null;
    const data = await res.json();
    if (data?.ok && Array.isArray(data.layout)) {
      return { layout: data.layout, isWorkspaceDefault: !!data.isWorkspaceDefault };
    }
  } catch {
    /* server or network unreachable */
  }
  return null;
}

/** Save layout to the server (PostgreSQL) */
export async function saveServerLayout(
  workspaceId: string,
  layout: WidgetLayoutItem[],
  asWorkspaceDefault = false
): Promise<boolean> {
  if (!workspaceId) return false;
  try {
    const res = await fetch(`/api/workspaces/${encodeURIComponent(workspaceId)}/layout`, {
      method: 'PUT',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ layout, asWorkspaceDefault }),
    });
    return res.ok;
  } catch {
    return false;
  }
}
