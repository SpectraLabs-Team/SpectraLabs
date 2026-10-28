'use client';

import { Info } from 'lucide-react';
import { cn } from '@/lib/utils';
import { usePortfolio } from './PortfolioContext';

/**
 * Says where pre-trade risk figures come from. The Risk Center reads the workspace's paper-order
 * book on the server; the sidebar's account is a separate simulation of the CSV portfolio kept in
 * this browser. Without this note the two look like one account that disagrees with itself.
 */
export function RiskSourceNote({ grossExposure, className }: { grossExposure: number | null | undefined; className?: string }) {
  const { positions } = usePortfolio();
  const emptyBook = !grossExposure;
  const csvLoaded = positions.length > 0;
  return (
    <p data-testid="risk-source-note" className={cn('flex items-start gap-2 text-[11.5px] leading-relaxed text-ink/55', className)}>
      <Info aria-hidden className="mt-0.5 h-3.5 w-3.5 shrink-0 text-ink/40" />
      <span>
        These figures use the positions and balance from <strong className="font-medium text-ink/75">paper orders placed in this workspace</strong>,
        stored on the server. The portfolio account in the sidebar is a separate simulation of your CSV portfolio, kept in this browser, and is not included.
        {emptyBook && csvLoaded && ' No paper orders have been placed yet, so exposure here is $0 even though the sidebar shows a loaded portfolio.'}
      </span>
    </p>
  );
}
