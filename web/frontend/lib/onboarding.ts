/**
 * Guided-tour state, shared by the client (components/onboarding), the API route
 * (app/api/auth/onboarding) and the tests (scripts/test-learn.mjs). No imports, so it runs anywhere.
 *
 * Authoritative store: the user_onboarding table, per signed-in user. localStorage keeps a copy
 * per user (keyed by email) only as a fallback for when the API cannot be reached, so the tour is
 * not shown again on every visit while the server is down.
 */

/** Bump when the tour changes enough that existing users should see it once more. */
export const TOUR_VERSION = 1;

export interface OnboardingState {
  onboardingCompleted: boolean;
  onboardingSkipped: boolean;
  /** The tour version the completed/skipped flags refer to (0 = never seen). */
  onboardingVersion: number;
  lastCompletedStep: string | null;
}

export type OnboardingAction = 'progress' | 'complete' | 'skip';

export const EMPTY_ONBOARDING: OnboardingState = {
  onboardingCompleted: false,
  onboardingSkipped: false,
  onboardingVersion: 0,
  lastCompletedStep: null,
};

export const STEP_ID = /^[a-z0-9-]{1,64}$/;

/** True when the tour should open by itself: the user has neither finished nor dismissed this version. */
export function shouldAutoStart(state: OnboardingState | null | undefined, version = TOUR_VERSION): boolean {
  if (!state) return true;
  if (state.onboardingVersion >= version && (state.onboardingCompleted || state.onboardingSkipped)) return false;
  return true;
}

/** Coerce anything (API JSON, localStorage) into a valid state. */
export function normalizeOnboarding(raw: unknown): OnboardingState {
  if (!raw || typeof raw !== 'object') return { ...EMPTY_ONBOARDING };
  const r = raw as Record<string, unknown>;
  const v = Number(r.onboardingVersion);
  const step = typeof r.lastCompletedStep === 'string' && STEP_ID.test(r.lastCompletedStep) ? r.lastCompletedStep : null;
  return {
    onboardingCompleted: r.onboardingCompleted === true,
    onboardingSkipped: r.onboardingSkipped === true,
    onboardingVersion: Number.isInteger(v) && v >= 0 && v <= 1000 ? v : 0,
    lastCompletedStep: step,
  };
}

/** The state after `action`, mirroring the SQL in app/api/auth/onboarding/route.ts. */
export function applyOnboarding(prev: OnboardingState, action: OnboardingAction, step: string | null, version = TOUR_VERSION): OnboardingState {
  switch (action) {
    case 'progress':
      return { ...prev, lastCompletedStep: step ?? prev.lastCompletedStep };
    case 'complete':
      return { onboardingCompleted: true, onboardingSkipped: false, onboardingVersion: Math.max(prev.onboardingVersion, version), lastCompletedStep: step ?? 'finish' };
    case 'skip':
      return { ...prev, onboardingSkipped: true, onboardingVersion: Math.max(prev.onboardingVersion, version), lastCompletedStep: step ?? prev.lastCompletedStep };
  }
}

/** Validate a PUT body. Returns the parsed request or an error message. */
export function parseOnboardingRequest(body: unknown): { action: OnboardingAction; step: string | null; version: number } | { error: string } {
  if (!body || typeof body !== 'object') return { error: 'Missing request body.' };
  const b = body as Record<string, unknown>;
  if (b.action !== 'progress' && b.action !== 'complete' && b.action !== 'skip') return { error: 'action must be progress, complete or skip.' };
  const step = b.step === undefined || b.step === null ? null : b.step;
  if (step !== null && (typeof step !== 'string' || !STEP_ID.test(step))) return { error: 'step must be a short id (a-z, 0-9, -).' };
  const version = b.version === undefined ? TOUR_VERSION : Number(b.version);
  if (!Number.isInteger(version) || version < 1 || version > 1000) return { error: 'version must be an integer from 1 to 1000.' };
  return { action: b.action, step: step as string | null, version };
}

/** localStorage key for one user's fallback copy. */
export function onboardingStorageKey(email: string): string {
  return `spectra-onboarding:${email.trim().toLowerCase()}`;
}
