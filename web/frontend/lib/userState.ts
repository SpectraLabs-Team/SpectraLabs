/**
 * Per-user saved state (M25.1). The server is the record; the browser copy is a cache that
 * paints the first frame and keeps the last values if the server cannot be reached.
 *
 * Routes: /api/v1/me/state, /api/v1/me/sandbox/orders, /api/v1/me/state/import.
 * The user and workspace come from the session on the server; nothing identifies the user here.
 */
import { ApiError, request } from './api';

/** The keys stored on the server. `sidebar` and the watchlist-open flag stay in the browser. */
export type StateKey = 'portfolio' | 'paper-account' | 'risk-params' | 'sandbox' | 'watchlist';

/** One saved value with its optimistic-lock version. Version 0 means nothing has been saved yet. */
export interface StateEntry<T> {
  value: T;
  version: number;
  updated_at: string | null;
}

export interface UserStateResponse {
  workspace_id: string;
  state: Record<StateKey, StateEntry<unknown>>;
}

export interface PortfolioValue {
  csv: string;
  source: string | null;
}

export interface SandboxValue {
  cash: number;
  positions: unknown[];
}

/** A stored sandbox order. `payload.id` is the id the screen uses; `id` is the server row id. */
export interface ServerOrder {
  id: string;
  payload: Record<string, unknown>;
  created_at: string;
}

const ME = '/api/v1/me';
/** Import key for this browser's one-time move (Decision 3). Repeat calls are no-ops on the server. */
const IMPORT_KEY = 'browser-v1';

export const getUserState = () => request<UserStateResponse>(`${ME}/state`);

export function putUserState<T>(key: StateKey, expectedVersion: number, value: T) {
  return request<{ value: T; version: number; updated_at: string }>(`${ME}/state/${key}`, {
    method: 'PUT',
    body: JSON.stringify({ expected_version: expectedVersion, value }),
  });
}

/** The server's current copy carried by a 409 (the save was refused), or null for any other error. */
export function conflictCurrent<T>(e: unknown): StateEntry<T> | null {
  if (!(e instanceof ApiError) || e.status !== 409) return null;
  return (e.details as { current?: StateEntry<T> } | undefined)?.current ?? null;
}

/** Oldest first, as the server returns them. */
export const listSandboxOrders = () =>
  request<{ orders: ServerOrder[] }>(`${ME}/sandbox/orders`).then((r) => r.orders);

export const appendSandboxOrder = (order: object) =>
  request<ServerOrder>(`${ME}/sandbox/orders`, { method: 'POST', body: JSON.stringify(order) });

export const updateSandboxOrder = (serverId: string, order: object) =>
  request<ServerOrder>(`${ME}/sandbox/orders/${serverId}`, { method: 'PATCH', body: JSON.stringify(order) });

export const clearSandboxOrders = () =>
  request<{ cleared: number }>(`${ME}/sandbox/orders`, { method: 'DELETE' });

export interface TransactResponse {
  saved: boolean;
  version: number;
  updated_at: string;
  value: SandboxValue;
  order: ServerOrder;
}

/** One sandbox trade: the order, new cash and positions, applied atomically. Refused with 409 if stale. */
export const transactSandbox = (body: { expected_version: number; cash: number; positions: unknown[]; order: object }) =>
  request<TransactResponse>(`${ME}/sandbox/transact`, { method: 'POST', body: JSON.stringify(body) });

export interface ImportReport {
  noop: boolean;
  imported: string[];
  skipped: string[];
}

export const importUserState = (importKey: string, data: Record<string, unknown>) =>
  request<ImportReport>(`${ME}/state/import`, {
    method: 'POST',
    body: JSON.stringify({ import_key: importKey, data }),
  });

/**
 * Browser keys that hold state from before M25.1. Portfolio, paper and risk are per account.
 * The sandbox and watchlist keys were never per account, so whoever signs in first on that browser
 * gets them. That is the behaviour the plan's one-time import describes; flagged for review.
 */
function legacyKeys(email: string): Record<StateKey, string> {
  return {
    portfolio: `spectra-portfolio-${email}`,
    'paper-account': `spectra-paper-${email}`,
    'risk-params': `spectra-risk-params-${email}`,
    sandbox: 'spectra-paper-sandbox-v1',
    watchlist: 'spectra-paper-watchlist',
  };
}

function readLegacy(email: string): Record<string, unknown> | null {
  const data: Record<string, unknown> = {};
  for (const [name, storageKey] of Object.entries(legacyKeys(email))) {
    try {
      const raw = localStorage.getItem(storageKey);
      if (!raw) continue;
      const parsed: unknown = JSON.parse(raw);
      const valid =
        name === 'watchlist'
          ? Array.isArray(parsed)
          : typeof parsed === 'object' && parsed !== null && !Array.isArray(parsed);
      if (valid) data[name] = parsed;
    } catch {
      /* unreadable legacy value: skip it */
    }
  }
  return Object.keys(data).length > 0 ? data : null;
}

/** Remove the legacy copies. Called only after the server has confirmed the import. */
function clearLegacy(email: string) {
  for (const storageKey of [...Object.values(legacyKeys(email)), 'spectra-alpaca-account-cache']) {
    try {
      localStorage.removeItem(storageKey);
    } catch {
      /* ignore */
    }
  }
}

const importing = new Map<string, Promise<void>>();

/**
 * Move this browser's old local state to the server, once. Concurrent callers share one run.
 * If the import fails, the next call tries again and the local copy stays in place.
 */
export function ensureImported(email: string): Promise<void> {
  const running = importing.get(email);
  if (running) return running;
  const job = (async () => {
    const legacy = readLegacy(email);
    if (!legacy) return;
    await importUserState(IMPORT_KEY, legacy);
    clearLegacy(email);
  })();
  importing.set(email, job);
  job.catch(() => importing.delete(email));
  return job;
}

const cacheKey = (email: string, name: string) => `spectra-cache:${email}:${name}`;

/** Cache read. Never the record: the server copy replaces it as soon as it loads. */
export function readCache<T>(email: string, name: string): T | null {
  try {
    const raw = localStorage.getItem(cacheKey(email, name));
    return raw ? (JSON.parse(raw) as T) : null;
  } catch {
    return null;
  }
}

/** Cache write, best-effort. */
export function writeCache(email: string, name: string, value: unknown): void {
  try {
    localStorage.setItem(cacheKey(email, name), JSON.stringify(value));
  } catch {
    /* the cache is best-effort */
  }
}

/**
 * Version-checked, debounced saves for one key.
 * A refused save (409) hands back the server's copy through `onConflict`, which the screen should apply.
 */
export class StateSaver<T> {
  private version = 0;
  private last: string | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private pending: { value: T } | null = null;

  constructor(
    private readonly key: StateKey,
    private readonly onConflict: (current: StateEntry<T>) => void,
    private readonly onError: (e: unknown) => void = () => undefined,
  ) {}

  /**
   * Set the baseline from the server copy the screen is showing. Pass `null` as the value when nothing
   * is saved yet, so the next save always goes out.
   */
  baseline(version: number, value: T | null) {
    this.version = version;
    this.last = value === null ? null : JSON.stringify(value);
  }

  /** Queue a save. A burst of changes goes out as one request. */
  schedule(value: T, delayMs = 400) {
    this.pending = { value };
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      void this.flush();
    }, delayMs);
  }

  async flush(): Promise<void> {
    const pending = this.pending;
    this.pending = null;
    if (!pending) return;
    const json = JSON.stringify(pending.value);
    if (json === this.last) return;
    try {
      const saved = await putUserState<T>(this.key, this.version, pending.value);
      this.version = saved.version;
      this.last = json;
    } catch (e) {
      const current = conflictCurrent<T>(e);
      if (current) {
        this.version = current.version;
        this.last = JSON.stringify(current.value);
        this.onConflict(current);
      } else {
        this.onError(e);
      }
    }
  }

  /** Stop the timer and send anything still pending, so a quick change is not lost on unmount. */
  dispose(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    void this.flush();
  }
}
