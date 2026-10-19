import { Logo } from '@/components/ui/primitives';
import { NavLink } from './NavLink';
import { DEV_LINKS, GITHUB_URL, NAV } from '@/lib/site';

/** GitHub mark (lucide-react 1.x dropped brand icons). */
function GithubIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" className={className}>
      <path d="M12 .5C5.65.5.5 5.65.5 12a11.5 11.5 0 0 0 7.86 10.92c.58.1.79-.25.79-.56v-2c-3.2.7-3.87-1.37-3.87-1.37-.52-1.33-1.28-1.69-1.28-1.69-1.05-.72.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.77 2.7 1.26 3.36.96.1-.75.4-1.26.73-1.55-2.55-.29-5.24-1.28-5.24-5.69 0-1.26.45-2.29 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.8 1.19 1.83 1.19 3.09 0 4.42-2.7 5.4-5.26 5.68.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 23.5 12C23.5 5.65 18.35.5 12 .5Z" />
    </svg>
  );
}

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
            <GithubIcon className="h-5 w-5" />
          </a>
        </div>
      </div>
      <div className="border-t border-ink/[0.05] px-5 py-4 text-center text-[11px] leading-5 text-ink/40 sm:px-8">
        Model note: the risk engine treats the book as one aggregate GBM position (zero drift, average volatility, no correlations). It is a defensible first model, not a production market-risk model.
      </div>
    </footer>
  );
}
