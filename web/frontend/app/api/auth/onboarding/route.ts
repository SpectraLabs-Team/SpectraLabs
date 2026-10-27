import { NextResponse } from 'next/server';
import { parseOnboardingRequest } from '@/lib/onboarding';
import { getOnboarding, recordOnboarding } from '@/lib/server/onboardingStore';
import { requireUser } from '@/lib/server/requireUser';

export const dynamic = 'force-dynamic';

/** GET: the signed-in user's guided-tour state (defaults when they have never seen it). */
export async function GET() {
  const { record, res } = await requireUser();
  if (res) return res;
  try {
    return NextResponse.json({ ok: true, onboarding: await getOnboarding(record.id) });
  } catch (err) {
    console.error('[ONBOARDING GET ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not load the tour state.' }, { status: 500 });
  }
}

/**
 * PUT {action: 'progress' | 'complete' | 'skip', step?, version?}: record tour progress for the
 * signed-in user only (lib/server/onboardingStore.ts). Changes nothing else about the account.
 */
export async function PUT(req: Request) {
  const { record, res } = await requireUser();
  if (res) return res;
  const parsed = parseOnboardingRequest(await req.json().catch(() => null));
  if ('error' in parsed) return NextResponse.json({ ok: false, error: parsed.error }, { status: 400 });
  try {
    return NextResponse.json({ ok: true, onboarding: await recordOnboarding(record.id, parsed.action, parsed.step, parsed.version) });
  } catch (err) {
    console.error('[ONBOARDING PUT ERROR]', err);
    return NextResponse.json({ ok: false, error: 'Could not save the tour state.' }, { status: 500 });
  }
}
