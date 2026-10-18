// The registry's chaining rule (crates/spectra-registry):
//   hash = SHA-256(prev + "\n" + json({seq, type, ts_ns, payload}))
// Computed in the browser for the landing page's ledger card. Events are illustrative.

export const GENESIS = '0'.repeat(64);

export interface ChainEvent {
  seq: number;
  type: string;
  payload: Record<string, unknown>;
}

export const DEMO_EVENTS: ChainEvent[] = [
  { seq: 0, type: 'experiment', payload: { name: 'sma_cross', hypothesis: 'trend persists' } },
  { seq: 1, type: 'run', payload: { trials: 48, dsr: 0.91 } },
  { seq: 2, type: 'artifact', payload: { file: 'equity.csv' } },
  { seq: 3, type: 'note', payload: { text: 'holdout untouched' } },
];

export async function sha256Hex(text: string): Promise<string | null> {
  if (typeof crypto === 'undefined' || !crypto.subtle) return null; // needs a secure context (https or localhost)
  const buf = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return Array.from(new Uint8Array(buf), (b) => b.toString(16).padStart(2, '0')).join('');
}

const line = (e: ChainEvent) => JSON.stringify({ seq: e.seq, type: e.type, ts_ns: 1_760_000_000_000 + e.seq, payload: e.payload });

/** Builds the chain, then verifies it by recomputing every link. */
export async function buildAndVerify(events = DEMO_EVENTS): Promise<{ head: string; verified: boolean } | null> {
  const hashes: string[] = [];
  let prev = GENESIS;
  for (const e of events) {
    const h = await sha256Hex(prev + '\n' + line(e));
    if (!h) return null;
    hashes.push(h);
    prev = h;
  }
  prev = GENESIS;
  let verified = true;
  for (const [i, e] of events.entries()) {
    if ((await sha256Hex(prev + '\n' + line(e))) !== hashes[i]) verified = false;
    prev = hashes[i];
  }
  return { head: hashes[hashes.length - 1], verified };
}
