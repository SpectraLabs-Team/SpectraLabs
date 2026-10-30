import { NextResponse } from 'next/server';
import { requireWorkspace } from '@/lib/server/requireUser';
import { putSecret, listSecrets, SecretValidationError, VaultConfigError } from '@/lib/server/vault';
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

    if (keyId.toUpperCase().startsWith('AK')) {
      return NextResponse.json(
        {
          ok: false,
          error:
            "It looks like you provided an Alpaca Live API key (starts with 'AK'). Paper trading requires Paper API keys (which start with 'PK'). Please get paper keys from your Alpaca Paper Trading dashboard.",
        },
        { status: 400 },
      );
    }

    // 1. Validate credentials live against Alpaca Paper API before persisting
    let testResp: Response;
    try {
      testResp = await fetch(`${ALPACA_PAPER_URL}/v2/account`, {
        headers: {
          'APCA-API-KEY-ID': keyId,
          'APCA-API-SECRET-KEY': secretKey,
          'User-Agent': 'SpectraLabs-Paper/1.0',
        },
        cache: 'no-store',
      });
    } catch (netErr) {
      const msg = netErr instanceof Error ? netErr.message : String(netErr);
      return NextResponse.json(
        { ok: false, error: `Could not reach Alpaca Paper API: ${msg}. Check your network connection.` },
        { status: 502 },
      );
    }

    if (!testResp.ok) {
      const errText = await testResp.text().catch(() => '');
      let errMsg = '';
      if (errText) {
        try {
          const parsed = JSON.parse(errText);
          if (parsed.message) errMsg = `Alpaca: ${parsed.message}`;
          else if (parsed.detail) errMsg = `Alpaca: ${parsed.detail}`;
        } catch {}
      }
      if (!errMsg) {
        if (testResp.status === 401 || testResp.status === 403) {
          errMsg = 'Alpaca authentication failed. Please double-check your API Key and Secret Key in your Alpaca Paper Trading dashboard.';
        } else {
          errMsg = `Alpaca paper API responded with status ${testResp.status}. Please verify your credentials.`;
        }
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
    if (err instanceof SecretValidationError) {
      return NextResponse.json({ ok: false, error: err.message }, { status: 400 });
    }
    if (err instanceof VaultConfigError) {
      return NextResponse.json(
        { ok: false, error: 'Key storage is not configured on this server (SECRETS_MASTER_KEY is missing or invalid).' },
        { status: 503 },
      );
    }
    const message = err instanceof Error ? err.message : String(err);
    console.error('[ALPACA CONNECT ERROR]', err);
    return NextResponse.json(
      { ok: false, error: `Could not connect Alpaca credentials: ${message}` },
      { status: 500 },
    );
  }
}
