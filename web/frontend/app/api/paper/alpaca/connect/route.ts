import { NextResponse } from 'next/server';
import { requireWorkspace } from '@/lib/server/requireUser';
import { putSecret, listSecrets } from '@/lib/server/vault';
import { record as audit, requestMeta } from '@/lib/server/audit';
import { clientIp, hit, tooMany } from '@/lib/server/rateLimit';

export const dynamic = 'force-dynamic';

const ALPACA_PAPER_URL = 'https://paper-api.alpaca.markets';

/** Connect or replace Alpaca paper credentials. Validates live against Alpaca before saving. */
export async function POST(req: Request) {
  const { auth, workspace, res } = await requireWorkspace('editor');
  if (res) return res;

  const limit = await hit(`alpaca-connect:${auth.user.id}:${clientIp(req)}`, 15, 60 * 60_000);
  if (!limit.ok) return tooMany(limit.retryAfter, 'connection attempts');

  try {
    const body = await req.json().catch(() => null);
    if (!body || typeof body !== 'object') {
      return NextResponse.json({ ok: false, error: 'Missing request body.' }, { status: 400 });
    }

    const keyId = typeof body.keyId === 'string' ? body.keyId.trim() : '';
    const secretKey = typeof body.secretKey === 'string' ? body.secretKey.trim() : '';

    if (!keyId || !secretKey) {
      return NextResponse.json(
        { ok: false, error: 'Both API Key ID and Secret Key are required.' },
        { status: 400 },
      );
    }

    if (keyId.length < 8 || secretKey.length < 16) {
      return NextResponse.json(
        { ok: false, error: 'Credentials do not match the expected format for Alpaca paper keys.' },
        { status: 400 },
      );
    }

    // 1. Validate credentials live against Alpaca Paper API before persisting
    const testResp = await fetch(`${ALPACA_PAPER_URL}/v2/account`, {
      headers: {
        'APCA-API-KEY-ID': keyId,
        'APCA-API-SECRET-KEY': secretKey,
      },
      cache: 'no-store',
    });

    if (!testResp.ok) {
      const errText = await testResp.text().catch(() => '');
      let errMsg = 'Invalid Alpaca paper trading credentials. Please verify your keys.';
      if (testResp.status === 401 || testResp.status === 403) {
        errMsg = 'Alpaca authentication failed. Please double-check your API Key and Secret Key.';
      } else if (errText.includes('message')) {
        try {
          const parsed = JSON.parse(errText);
          if (parsed.message) errMsg = `Alpaca: ${parsed.message}`;
        } catch {}
      }
      return NextResponse.json({ ok: false, error: errMsg }, { status: 400 });
    }

    const account = await testResp.json();

    // 2. Persist with AES-256-GCM envelope encryption bound to this workspace
    const existed = (await listSecrets(workspace.id)).some((s) => s.kind === 'alpaca_paper');
    const secretInfo = await putSecret(workspace.id, 'alpaca_paper', { keyId, secretKey }, auth.user.id);

    await audit(
      {
        action: existed ? 'secret.replaced' : 'secret.added',
        actor: auth.user,
        workspaceId: workspace.id,
        target: { type: 'secret', id: 'alpaca_paper' },
        details: { hint: secretInfo.hint, broker: 'alpaca_paper' },
      },
      requestMeta(req),
    );

    return NextResponse.json({
      ok: true,
      connected: true,
      hint: secretInfo.hint,
      keyIdHint: secretInfo.hint,
      account,
    });
  } catch (err) {
    console.error('[ALPACA CONNECT ERROR]', err instanceof Error ? err.name : 'unknown');
    return NextResponse.json(
      { ok: false, error: 'Could not connect Alpaca credentials. Please try again.' },
      { status: 500 },
    );
  }
}
