import Link from 'next/link';
import { Logo } from '@/components/ui/primitives';
import { GITHUB_URL } from '@/lib/site';

const cols = [
  {
    title: 'Product',
    links: [
      { label: 'Risk & Stress Lab', href: '/studio?tab=risk' },
      { label: 'Portfolio Studio', href: '/studio?tab=portfolio' },
      { label: 'Hardware Benchmarks', href: '/studio?tab=benchmark' },
    ],
  },
  {
    title: 'Platform',
    links: [
      { label: 'Features', href: '/#features' },
      { label: 'Benchmarks', href: '/#benchmarks' },
      { label: 'Architecture', href: '/#architecture' },
    ],
  },
  {
    title: 'Source',
    links: [
      { label: 'GitHub', href: GITHUB_URL },
      { label: 'Porting notes', href: `${GITHUB_URL}/blob/main/PORTING.md` },
      { label: 'Known issues', href: `${GITHUB_URL}/blob/main/KNOWN_ISSUES.md` },
    ],
  },
];

export function Footer() {
  return (
    <footer className="border-t border-white/[0.06] bg-obsidian-900/40">
      <div className="mx-auto grid max-w-7xl gap-10 px-5 py-14 sm:px-8 md:grid-cols-[1.4fr_repeat(3,1fr)]">
        <div>
          <Logo />
          <p className="mt-4 max-w-xs text-sm leading-6 text-slate-500">Evidence-first quantitative research and portfolio risk, in pure Rust.</p>
          <p className="mt-4 max-w-sm text-[11.5px] leading-5 text-slate-600">
            Model note: the risk engine treats the book as one aggregate GBM position (zero drift, average volatility, no correlations). It is a defensible first model, not a production market-risk model.
          </p>
        </div>
        {cols.map((c) => (
          <div key={c.title}>
            <p className="font-mono text-[10.5px] uppercase tracking-[0.16em] text-slate-500">{c.title}</p>
            <ul className="mt-4 space-y-2.5">
              {c.links.map((l) => (
                <li key={l.label}>
                  {l.href.startsWith('http') ? (
                    <a href={l.href} target="_blank" rel="noreferrer" className="text-sm text-slate-400 transition hover:text-white">
                      {l.label}
                    </a>
                  ) : (
                    <Link href={l.href} className="text-sm text-slate-400 transition hover:text-white">
                      {l.label}
                    </Link>
                  )}
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
      <div className="border-t border-white/[0.05]">
        <div className="mx-auto flex max-w-7xl flex-wrap items-center justify-between gap-3 px-5 py-5 font-mono text-[11px] text-slate-600 sm:px-8">
          <span>© {new Date().getFullYear()} Spectra Labs · Proprietary, all rights reserved</span>
          <span>Rust 1.89+ · std only · unsafe forbidden</span>
        </div>
      </div>
    </footer>
  );
}
