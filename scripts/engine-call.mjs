#!/usr/bin/env node
// Call the engine API directly with a valid proxy signature, for testing and operations.
// The dashboard's proxy normally does this (web/frontend/lib/server/engineSign.ts); the canonical
// string must match `canonical` in crates/spectra-api/src/auth.rs byte for byte.
//
//   ENGINE_SECRET=... node scripts/engine-call.mjs METHOD PATH [JSON_BODY]
//
//   ENGINE_URL        default http://127.0.0.1:8000
//   SPECTRA_USER      the acting user's id (default 'engine-call': jobs then have no created_by)
//   SPECTRA_WORKSPACE the workspace id the call acts in (needed for /api/v1/jobs)
//
// Example: queue a risk job and read it back
//   node scripts/engine-call.mjs POST /api/v1/jobs '{"kind":"risk","payload":{"portfolio_csv":"symbol,quantity,price\nAAPL,10,190"}}'
//   node scripts/engine-call.mjs GET /api/v1/jobs/<id>
import crypto from 'node:crypto';

const [method = 'GET', path, body = ''] = process.argv.slice(2);
if (!path || !path.startsWith('/')) {
  console.error('usage: node scripts/engine-call.mjs METHOD /path [JSON_BODY]');
  process.exit(2);
}
const secret = process.env.ENGINE_SECRET;
if (!secret) {
  console.error('engine-call: set ENGINE_SECRET (the same value the API uses)');
  process.exit(2);
}
const base = (process.env.ENGINE_URL || 'http://127.0.0.1:8000').replace(/\/$/, '');
const user = process.env.SPECTRA_USER || 'engine-call';
const workspace = process.env.SPECTRA_WORKSPACE || '';
const requestId = `engine-call-${crypto.randomUUID()}`;
const timestamp = String(Math.floor(Date.now() / 1000));
const bodyHash = crypto.createHash('sha256').update(body, 'utf8').digest('hex');
const canonical = ['spectra-v1', method.toUpperCase(), path, timestamp, user, workspace, requestId, bodyHash].join('\n');
const signature = crypto.createHmac('sha256', secret).update(canonical, 'utf8').digest('hex');

const res = await fetch(base + path, {
  method: method.toUpperCase(),
  headers: {
    ...(body ? { 'content-type': 'application/json' } : {}),
    'x-spectra-user': user,
    'x-spectra-workspace': workspace,
    'x-request-id': requestId,
    'x-spectra-timestamp': timestamp,
    'x-spectra-signature': signature,
  },
  body: body || undefined,
});
const text = await res.text();
console.error(`${res.status} ${res.statusText}  (x-request-id: ${res.headers.get('x-request-id')})`);
try {
  console.log(JSON.stringify(JSON.parse(text), null, 2));
} catch {
  console.log(text);
}
process.exit(res.ok ? 0 : 1);
