import { Github } from 'lucide-react';
import { Logo } from '@/components/ui/primitives';
import { NavLink } from './NavLink';
import { DEV_LINKS, GITHUB_URL, NAV } from '@/lib/site';

export function Footer() {
  return (
    <footer className="border-t border-ink/[0.07] bg-panel/40">
      <div className="mx-auto flex max-w-[1320px] flex-col gap-6 px-5 py-8 sm:px-8 lg:flex-row lg:items-center">
        <div>
          <Logo />
          <p className="mt-1.5 pl-[38px] text-xs text-ink/50">Evidence-First Quantitative Research</p>
        </div>
        <nav aria-label="Footer" className="flex flex-wrap gap-x-6 gap-y-2 lg:mx-auto">
          {[...NAV, ...DEV_LINKS].map((l) => (
            <NavLink key={l.label} href={l.href} external={l.external} className="text-[13px] text-ink/60 transition hover:text-ink">
              {l.label}
            </NavLink>
          ))}
        </nav>
        <div className="flex flex-wrap items-center gap-x-5 gap-y-2 font-mono text-[12px] text-ink/50">
          <span>v0.7.0</span>
          <span>Proprietary · all rights reserved</span>
          <span>Built with Rust</span>
          <a href={GITHUB_URL} target="_blank" rel="noreferrer" aria-label="GitHub" className="text-ink/60 transition hover:text-ink">
            <Github className="h-5 w-5" />
          </a>
        </div>
      </div>
      <div className="border-t border-ink/[0.05] px-5 py-4 text-center text-[11px] leading-5 text-ink/40 sm:px-8">
        Model note: the risk engine treats the book as one aggregate GBM position (zero drift, average volatility, no correlations). It is a defensible first model, not a production market-risk model.
      </div>
    </footer>
  );
}
