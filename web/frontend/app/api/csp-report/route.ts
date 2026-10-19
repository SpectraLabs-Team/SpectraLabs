import { NextResponse } from 'next/server';
import { clientIp, hit } from '@/lib/server/rateLimit';

export const dynamic = 'force-dynamic';

const MAX_BODY = 16 * 1024;

/** Page URLs can carry secrets: drop query strings and invite tokens before logging. */
const redact = (url: string) => url.split(/[?#]/)[0].replace(/\/invite\/[^/]+/, '/invite/…');

/**
 * Content-Security-Policy violation reports (M19 task 14). Browsers send these on their own, in the
 * old report-uri format ({"csp-report": {...}}) or the Reporting API format ([{type, body}]).
 * Logged in a compact form so a report-only rollout shows what an enforced policy would block.
 * Rate-limited per IP; always answers 204 (a browser does nothing with the reply).
 */
export async function POST(req: Request) {
  const limited = !(await hit(`csp-report:${clientIp(req)}`, 100, 10 * 60_000)).ok;
  const text = await req.text().catch(() => '');
  if (limited || !text || text.length > MAX_BODY) return new NextResponse(null, { status: 204 });
  try {
    const data = JSON.parse(text);
    const reports: Record<string, unknown>[] = Array.isArray(data)
      ? data.filter((r) => r?.type === 'csp-violation').map((r) => r.body ?? {})
      : data?.['csp-report']
        ? [data['csp-report']]
        : [];
    for (const r of reports.slice(0, 10)) {
      const pick = (...keys: string[]) => keys.map((k) => r[k]).find((v) => typeof v === 'string') as string | undefined;
      const page = pick('document-uri', 'documentURL');
      console.warn(
        '[CSP]',
        JSON.stringify({
          directive: pick('effective-directive', 'effectiveDirective', 'violated-directive'),
          blocked: pick('blocked-uri', 'blockedURL'),
          page: page ? redact(page) : undefined,
          source: pick('source-file', 'sourceFile'),
          line: r['line-number'] ?? r.lineNumber,
          disposition: pick('disposition'),
        }),
      );
    }
  } catch {
    /* not JSON: ignore */
  }
  return new NextResponse(null, { status: 204 });
}
