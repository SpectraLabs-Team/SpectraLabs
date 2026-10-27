import { EMPTY_ONBOARDING, type OnboardingAction, type OnboardingState } from '../onboarding';
import { query } from './db';

/**
 * Per-user guided-tour state (db/migrations/012_onboarding.sql). Server-only.
 * Every query is keyed by the caller's own user id, so one user's tour never affects another's.
 * Keep the SQL in step with applyOnboarding in lib/onboarding.ts.
 */

type Row = { completed: boolean; skipped: boolean; version: number; last_step: string | null };

const toState = (r: Row | undefined): OnboardingState =>
  r ? { onboardingCompleted: r.completed, onboardingSkipped: r.skipped, onboardingVersion: r.version, lastCompletedStep: r.last_step } : { ...EMPTY_ONBOARDING };

export async function getOnboarding(userId: string): Promise<OnboardingState> {
  const rows = await query<Row>('SELECT completed, skipped, version, last_step FROM user_onboarding WHERE user_id = $1', [userId]);
  return toState(rows[0]);
}

const RETURNING = 'RETURNING completed, skipped, version, last_step';

/**
 *   progress: remember the last step reached (flags unchanged)
 *   complete: finished `version` (clears skipped)
 *   skip:     dismissed `version` (a finished tour stays finished)
 * The stored version never goes down.
 */
export async function recordOnboarding(userId: string, action: OnboardingAction, step: string | null, version: number): Promise<OnboardingState> {
  let rows: Row[];
  if (action === 'progress') {
    rows = await query<Row>(
      `INSERT INTO user_onboarding (user_id, last_step) VALUES ($1, $2)
       ON CONFLICT (user_id) DO UPDATE SET last_step = COALESCE($2, user_onboarding.last_step), updated_at = now()
       ${RETURNING}`,
      [userId, step],
    );
  } else if (action === 'complete') {
    rows = await query<Row>(
      `INSERT INTO user_onboarding (user_id, completed, skipped, version, last_step) VALUES ($1, true, false, $3, COALESCE($2::text, 'finish'))
       ON CONFLICT (user_id) DO UPDATE SET completed = true, skipped = false, version = GREATEST(user_onboarding.version, $3),
         last_step = COALESCE($2::text, 'finish'), updated_at = now()
       ${RETURNING}`,
      [userId, step, version],
    );
  } else {
    rows = await query<Row>(
      `INSERT INTO user_onboarding (user_id, skipped, version, last_step) VALUES ($1, true, $3, $2)
       ON CONFLICT (user_id) DO UPDATE SET skipped = true, version = GREATEST(user_onboarding.version, $3),
         last_step = COALESCE($2, user_onboarding.last_step), updated_at = now()
       ${RETURNING}`,
      [userId, step, version],
    );
  }
  return toState(rows[0]);
}
