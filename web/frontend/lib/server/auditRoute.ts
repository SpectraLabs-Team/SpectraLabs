import { NextResponse } from 'next/server';
import { filterFromQuery, listAudit, record, requestMeta, toCsv, verifyChain, type AuditFilter } from './audit';

const CSV_MAX_ROWS = 50_000;

/**
 * Shared GET handler for the admin and workspace audit views: JSON pages (newest first, `before`
 * cursor) or, with ?format=csv, a CSV download of everything matching the filters (up to 50,000
 * rows). Exports are themselves audit-logged, and so is opening the log or changing its filters
 * (`audit.viewed`); loading further pages (a `before` cursor) is not, to keep the log readable.
 * The chain head hash travels in X-Audit-Head.
 */
export async function auditResponse(
  req: Request,
  scope: { workspaceId?: string },
  actor: { id: string; email: string },
): Promise<NextResponse> {
  const sp = new URL(req.url).searchParams;
  const filter: AuditFilter = { ...filterFromQuery(sp), ...scope };

  if (sp.get('format') !== 'csv') {
    const limit = Math.min(filter.limit ?? 50, 200);
    const events = await listAudit({ ...filter, limit: limit + 1 });
    const more = events.length > limit;
    const page = events.slice(0, limit);
    if (!filter.beforeSeq) {
      await record(
        {
          action: 'audit.viewed',
          actor,
          workspaceId: scope.workspaceId ?? null,
          details: { rows: page.length, filters: Object.fromEntries([...sp.entries()].filter(([k]) => k !== 'before')) },
        },
        requestMeta(req),
      );
    }
    return NextResponse.json({ ok: true, events: page, nextBefore: more ? page[page.length - 1].seq : null });
  }

  const rows = [];
  let before = filter.beforeSeq;
  while (rows.length < CSV_MAX_ROWS) {
    const batch = await listAudit({ ...filter, beforeSeq: before, limit: 1000 });
    rows.push(...batch);
    if (batch.length < 1000) break;
    before = batch[batch.length - 1].seq;
  }
  const { head } = await verifyChain().catch(() => ({ head: null }));
  await record(
    {
      action: 'audit.exported',
      actor,
      workspaceId: scope.workspaceId ?? null,
      details: { rows: rows.length, filters: Object.fromEntries([...sp.entries()].filter(([k]) => k !== 'format')) },
    },
    requestMeta(req),
  );
  const stamp = new Date().toISOString().slice(0, 10);
  return new NextResponse(toCsv(rows), {
    headers: {
      'Content-Type': 'text/csv; charset=utf-8',
      'Content-Disposition': `attachment; filename="spectra-audit-${scope.workspaceId ? 'workspace-' : ''}${stamp}.csv"`,
      'Cache-Control': 'no-store',
      ...(head ? { 'X-Audit-Head': head } : {}),
    },
  });
}
