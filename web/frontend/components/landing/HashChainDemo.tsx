'use client';

import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { Link2, ShieldAlert, ShieldCheck } from 'lucide-react';
import { cn } from '@/lib/utils';

// Illustrates the registry's chaining rule (crates/spectra-registry):
//   hash = SHA-256(prev + "\n" + json({seq, type, ts_ns, payload}))
// Each event commits to the previous hash: an edited event no longer matches its stored hash, and
// rewriting that hash breaks the next event's `prev` link. Events here are illustrative.
const EVENTS = [
  { seq: 0, type: 'experiment', payload: { name: 'sma_cross', hypothesis: 'trend persists' } },
  { seq: 1, type: 'run', payload: { trials: 48, dsr: 0.91 } },
  { seq: 2, type: 'artifact', payload: { file: 'equity.csv' } },
  { seq: 3, type: 'note', payload: { text: 'holdout untouched' } },
];

async function sha256(text: string): Promise<string> {
  if (typeof crypto === 'undefined' || !crypto.subtle) return '—'.repeat(8);
  const buf = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return Array.from(new Uint8Array(buf), (b) => b.toString(16).padStart(2, '0')).join('');
}

const GENESIS = '0'.repeat(64);

export function HashChainDemo() {
  const [tampered, setTampered] = useState(false);
  const [chain, setChain] = useState<{ stored: string; recomputed: string; prevOk: boolean }[]>([]);

  useEffect(() => {
    let active = true;
    (async () => {
      // Hashes as originally written.
      const stored: string[] = [];
      let prev = GENESIS;
      for (const e of EVENTS) {
        prev = await sha256(prev + '\n' + JSON.stringify({ seq: e.seq, type: e.type, ts_ns: 1_760_000_000_000 + e.seq, payload: e.payload }));
        stored.push(prev);
      }
      // Verification pass over the (possibly edited) log.
      const rows: { stored: string; recomputed: string; prevOk: boolean }[] = [];
      prev = GENESIS;
      for (const [i, e] of EVENTS.entries()) {
        const payload = tampered && i === 1 ? { ...e.payload, dsr: 0.99 } : e.payload;
        const recomputed = await sha256(prev + '\n' + JSON.stringify({ seq: e.seq, type: e.type, ts_ns: 1_760_000_000_000 + e.seq, payload }));
        rows.push({ stored: stored[i], recomputed, prevOk: i === 0 || rows[i - 1].stored === rows[i - 1].recomputed });
        prev = stored[i];
      }
      if (active) setChain(rows);
    })();
    return () => {
      active = false;
    };
  }, [tampered]);

  const firstBad = chain.findIndex((r) => r.stored !== r.recomputed);

  return (
    <div className="mt-5">
      <div className="flex flex-col gap-1.5">
        {EVENTS.map((e, i) => {
          const row = chain[i];
          const bad = row && row.stored !== row.recomputed;
          return (
            <motion.div key={e.seq} layout className={cn('flex items-center gap-3 rounded-lg border px-3 py-2 font-mono text-[11px] transition-colors', bad ? 'border-loss/40 bg-loss/[0.07]' : 'border-white/[0.07] bg-obsidian-950/60')}>
              <span className="w-5 text-slate-600">#{e.seq}</span>
              <span className="w-16 text-slate-300">{e.type}</span>
              <span className={cn('hidden flex-1 truncate sm:block', tampered && i === 1 ? 'text-caution' : 'text-slate-500')}>
                {JSON.stringify(tampered && i === 1 ? { ...e.payload, dsr: 0.99 } : e.payload)}
              </span>
              <span className={cn('ml-auto flex items-center gap-1', bad ? 'text-loss' : 'text-accent')}>
                <Link2 className="h-3 w-3" />
                {row ? (bad ? row.recomputed : row.stored).slice(0, 10) : '··········'}
              </span>
            </motion.div>
          );
        })}
      </div>
      <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
        <span className={cn('flex items-center gap-1.5 text-xs', firstBad >= 0 ? 'text-loss' : 'text-profit')}>
          {firstBad >= 0 ? <ShieldAlert className="h-3.5 w-3.5" /> : <ShieldCheck className="h-3.5 w-3.5" />}
          {firstBad >= 0 ? `verify(): chain broken at event #${firstBad}` : 'verify(): chain intact'}
        </span>
        <button type="button" onClick={() => setTampered((t) => !t)} className="btn-ghost border border-white/10">
          {tampered ? 'Restore log' : 'Tamper with run #1'}
        </button>
      </div>
    </div>
  );
}
