'use client';

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { getSamplePortfolio } from '@/lib/api';
import { SAMPLE_CSV, parseCsv, portfolioStats, toCsv, type PortfolioStats } from '@/lib/portfolio';
import type { Position } from '@/lib/types';
import { useEngineHealth, type EngineState } from '@/lib/useEngineHealth';

type Source = 'engine sample' | 'bundled sample' | 'uploaded' | 'pasted' | 'edited';

interface PortfolioCtx {
  positions: Position[];
  csv: string;
  stats: PortfolioStats;
  source: Source;
  /** Bumps whenever positions change, so consumers can re-run engine calls. */
  revision: number;
  engine: EngineState;
  setPositions: (p: Position[], source: Source) => void;
  loadSample: () => Promise<void>;
  importCsv: (text: string, source: 'uploaded' | 'pasted') => string[];
}

const Ctx = createContext<PortfolioCtx | null>(null);

export function PortfolioProvider({ children }: { children: ReactNode }) {
  const engine = useEngineHealth();
  const [positions, setPos] = useState<Position[]>(() => parseCsv(SAMPLE_CSV).positions);
  const [source, setSource] = useState<Source>('bundled sample');
  const [revision, setRevision] = useState(0);

  const setPositions = useCallback((p: Position[], s: Source) => {
    setPos(p);
    setSource(s);
    setRevision((r) => r + 1);
  }, []);

  const loadSample = useCallback(async () => {
    try {
      setPositions(parseCsv(await getSamplePortfolio()).positions, 'engine sample');
    } catch {
      setPositions(parseCsv(SAMPLE_CSV).positions, 'bundled sample');
    }
  }, [setPositions]);

  // Swap in the engine's sample once it is reachable, unless the user already changed the book.
  const online = engine.status === 'online';
  useEffect(() => {
    if (online && source === 'bundled sample') loadSample();
  }, [online, source, loadSample]);

  const importCsv = useCallback(
    (text: string, s: 'uploaded' | 'pasted') => {
      const { positions: p, errors } = parseCsv(text);
      if (!errors.length && p.length) setPositions(p, s);
      else if (!p.length && !errors.length) return ['No positions found. Expected a header row, then symbol,quantity,price[,volatility].'];
      return errors;
    },
    [setPositions],
  );

  const value = useMemo<PortfolioCtx>(
    () => ({ positions, csv: toCsv(positions), stats: portfolioStats(positions), source, revision, engine, setPositions, loadSample, importCsv }),
    [positions, source, revision, engine, setPositions, loadSample, importCsv],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function usePortfolio() {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error('usePortfolio must be used inside <PortfolioProvider>');
  return ctx;
}
