import { CheckCheck, Database, Lock, ShieldCheck, Zap } from 'lucide-react';
import { HEADLINE } from '@/lib/baseline';
import { formatDuration } from '@/lib/utils';

const ITEMS = [
  { icon: Zap, title: '< 1 ms', sub: `10k-path VaR (${HEADLINE.smallMs ? formatDuration(HEADLINE.smallMs) : '—'} measured)` },
  { icon: Database, title: 'Zero', sub: 'Third-party dependencies' },
  { icon: ShieldCheck, title: 'Unsafe forbidden', sub: 'Memory-safe Rust workspace' },
  { icon: CheckCheck, title: 'Bit-exact', sub: 'Parity with the C++ reference' },
  { icon: Lock, title: 'Proprietary', sub: 'Self-hosted, your data stays local' },
];

export function TrustBar() {
  return (
    <section className="py-12" aria-label="Engineering guarantees">
      <div className="mx-auto max-w-[1320px] px-5 sm:px-8">
        <p className="font-mono text-[11.5px] font-medium uppercase tracking-[0.22em] text-ink/60">Built to be audited</p>
        <div className="mt-6 grid gap-6 sm:grid-cols-2 lg:grid-cols-5 lg:divide-x lg:divide-ink/[0.08]">
          {ITEMS.map((it) => (
            <div key={it.title} className="flex items-center gap-4 lg:px-5 lg:first:pl-0">
              <span className="flex h-12 w-12 shrink-0 items-center justify-center rounded-full bg-accent/10 text-accent ring-1 ring-accent/25">
                <it.icon className="h-6 w-6" />
              </span>
              <div>
                <p className="text-[15px] font-semibold text-ink">{it.title}</p>
                <p className="text-[12.5px] leading-5 text-ink/55">{it.sub}</p>
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
