'use client';

import { useCallback } from 'react';
import { usePathname, useRouter, useSearchParams } from 'next/navigation';
import { AnimatePresence, motion } from 'framer-motion';
import { BacktestLab } from './BacktestLab';
import { BenchmarkLab } from './BenchmarkLab';
import { ExperimentRegistry } from './ExperimentRegistry';
import { DataLab } from './DataLab';
import { MarketSettings } from './MarketSettings';
import { OptimizationLab } from './OptimizationLab';
import { OverviewDashboard } from './OverviewDashboard';
import { PaperLab } from './PaperLab';
import { PortfolioProvider } from './PortfolioContext';
import { PortfolioStudio } from './PortfolioStudio';
import { RiskCenter } from './RiskCenter';
import { RiskLab } from './RiskLab';
import { StrategyStudio } from './StrategyStudio';
import { OnboardingProvider } from '@/components/onboarding/OnboardingTour';
import { StudioShell, TABS, type StudioTab } from './StudioShell';

const TAB_IDS = TABS.map((t) => t.id);

export function StudioApp() {
  const params = useSearchParams();
  const router = useRouter();
  const pathname = usePathname();
  const raw = params.get('tab');
  const tab: StudioTab = TABS.some((t) => t.id === raw) ? (raw as StudioTab) : 'overview';

  const setTab = useCallback((t: StudioTab) => router.replace(`${pathname}?tab=${t}`, { scroll: false }), [router, pathname]);
  const selectRun = useCallback((runId: string) => router.replace(`${pathname}?tab=backtests&run=${encodeURIComponent(runId)}`, { scroll: false }), [router, pathname]);

  return (
    <PortfolioProvider>
      <OnboardingProvider tab={tab} tabs={TAB_IDS} onTab={setTab}>
      <StudioShell tab={tab} onTab={setTab}>
        <AnimatePresence mode="wait" initial={false}>
          <motion.div key={tab} initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -4 }} transition={{ duration: 0.18 }}>
            {tab === 'overview' && <OverviewDashboard onTab={setTab} onSelectRun={selectRun} />}
            {tab === 'risk' && <RiskLab />}
            {tab === 'risk-center' && <RiskCenter />}
            {tab === 'paper' && <PaperLab />}
            {tab === 'portfolio' && <PortfolioStudio />}
            {tab === 'data' && <DataLab />}
            {tab === 'strategies' && <StrategyStudio />}
            {tab === 'backtests' && <BacktestLab />}
            {tab === 'optimization' && <OptimizationLab />}
            {tab === 'benchmark' && <BenchmarkLab />}
            {tab === 'registry' && <ExperimentRegistry />}
            {tab === 'settings' && <MarketSettings />}
          </motion.div>
        </AnimatePresence>
      </StudioShell>
      </OnboardingProvider>
    </PortfolioProvider>
  );
}
