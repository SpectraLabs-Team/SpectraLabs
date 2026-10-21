'use client';

import { useCallback } from 'react';
import { usePathname, useRouter, useSearchParams } from 'next/navigation';
import { AnimatePresence, motion } from 'framer-motion';
import { BacktestLab } from './BacktestLab';
import { BenchmarkLab } from './BenchmarkLab';
import { ExperimentRegistry } from './ExperimentRegistry';
import { DataLab } from './DataLab';
import { MarketSettings } from './MarketSettings';
import { PortfolioProvider } from './PortfolioContext';
import { PortfolioStudio } from './PortfolioStudio';
import { RiskLab } from './RiskLab';
import { StudioShell, TABS, type StudioTab } from './StudioShell';

export function StudioApp() {
  const params = useSearchParams();
  const router = useRouter();
  const pathname = usePathname();
  const raw = params.get('tab');
  const tab: StudioTab = TABS.some((t) => t.id === raw) ? (raw as StudioTab) : 'risk';

  const setTab = useCallback((t: StudioTab) => router.replace(`${pathname}?tab=${t}`, { scroll: false }), [router, pathname]);

  return (
    <PortfolioProvider>
      <StudioShell tab={tab} onTab={setTab}>
        <AnimatePresence mode="wait" initial={false}>
          <motion.div key={tab} initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -4 }} transition={{ duration: 0.18 }}>
            {tab === 'risk' && <RiskLab />}
            {tab === 'portfolio' && <PortfolioStudio />}
            {tab === 'data' && <DataLab />}
            {tab === 'backtests' && <BacktestLab />}
            {tab === 'benchmark' && <BenchmarkLab />}
            {tab === 'registry' && <ExperimentRegistry />}
            {tab === 'settings' && <MarketSettings />}
          </motion.div>
        </AnimatePresence>
      </StudioShell>
    </PortfolioProvider>
  );
}
