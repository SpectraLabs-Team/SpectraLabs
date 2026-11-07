'use client';

import { useCallback } from 'react';
import { usePathname, useRouter, useSearchParams } from 'next/navigation';
import { Briefcase, PieChart } from 'lucide-react';
import { PortfolioLab } from './PortfolioLab';
import { PortfolioStudio } from './PortfolioStudio';

export type PortfolioView = 'positions' | 'saved';

/** The sub-view a `view` URL parameter selects: `saved` opens saved portfolios (M15), anything else the positions. */
export function portfolioView(param: string | null): PortfolioView {
  return param === 'saved' ? 'saved' : 'positions';
}

/**
 * Studio → Portfolio: the positions and risk view (Portfolio Studio), and saved portfolios with
 * target weights and their backtests (M15). The sub-view is kept in the URL (`?tab=portfolio&view=saved`).
 */
export function PortfolioWorkspace() {
  const params = useSearchParams();
  const router = useRouter();
  const pathname = usePathname();
  const view = portfolioView(params.get('view'));
  const select = useCallback(
    (v: PortfolioView) => router.replace(`${pathname}?tab=portfolio${v === 'saved' ? '&view=saved' : ''}`, { scroll: false }),
    [router, pathname],
  );
  const tab = (v: PortfolioView, label: string, Icon: typeof Briefcase) => (
    <button
      type="button"
      onClick={() => select(v)}
      aria-pressed={view === v}
      className={`inline-flex items-center gap-2 rounded-lg px-3.5 py-2 text-xs font-medium transition ${
        view === v ? 'bg-accent text-white shadow-sm' : 'bg-panel/40 text-ink/70 hover:bg-panel hover:text-ink'
      }`}
    >
      <Icon className="h-4 w-4" />
      {label}
    </button>
  );
  return (
    <div className="space-y-5">
      <div className="flex items-center gap-2 border-b border-ink/10 pb-3">
        {tab('positions', 'Positions & Risk', PieChart)}
        {tab('saved', 'Saved Portfolios & Backtests', Briefcase)}
      </div>
      {view === 'saved' ? <PortfolioLab /> : <PortfolioStudio />}
    </div>
  );
}
