/**
 * Browser-side rule for when to send an activity heartbeat. Pure, so it can be tested without a DOM.
 * The server applies its own limits (see lib/server/activity.ts); this only avoids sending
 * heartbeats when nobody is using the page.
 */

/** No input for this long and the page counts as idle. */
export const INACTIVITY_MS = 5 * 60_000;
/** How often a heartbeat is sent. Matches the server's credit cap (60 seconds). */
export const HEARTBEAT_MS = 60_000;

export function isActive(state: { visible: boolean; lastInputAt: number; now: number }): boolean {
  return state.visible && state.now - state.lastInputAt <= INACTIVITY_MS && state.now >= state.lastInputAt;
}

/**
 * Days daily activity totals are kept, as told to users on the Account page. Must equal
 * ACTIVITY_RETENTION_DAYS in lib/server/activity.ts (test-activity.mjs checks this).
 */
export const ACTIVITY_RETENTION_DAYS_CLIENT = 400;
