import { NextResponse } from 'next/server';
import { AppUrlError } from './appUrl';
import { TeamError } from './team';

/** Turn team errors into clean JSON answers; anything else is a logged 500. */
export function teamErrorResponse(err: unknown, what: string): NextResponse {
  if (err instanceof TeamError) return NextResponse.json({ ok: false, error: err.message }, { status: err.status });
  if (err instanceof AppUrlError) return NextResponse.json({ ok: false, error: 'This server can’t send invite links yet (APP_URL is not set).' }, { status: 503 });
  console.error(`[TEAM ERROR] ${what}:`, err instanceof Error ? err.message : err);
  return NextResponse.json({ ok: false, error: `Could not ${what}.` }, { status: 500 });
}
