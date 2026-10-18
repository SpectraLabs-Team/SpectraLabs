'use client';

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useAuth } from '@/lib/auth';
import { getSamplePortfolio } from '@/lib/api';
import { DEFAULT_PAPER, paperAccount, type PaperAccount, type PaperSettings } from '@/lib/paperLedger';
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
  paper: PaperSettings;
  account: PaperAccount;
  setPaper: (p: PaperSettings) => void;
  setPositions: (p: Position[], source: Source) => void;
  loadSample: () => Promise<void>;
  importCsv: (text: string, source: 'uploaded' | 'pasted') => string[];
}

const Ctx = createContext<PortfolioCtx | null>(null);
/** Per-account storage keys, so two people on one browser never see each other's book. */
const portfolioKey = (email?: string) => `spectra-portfolio-${email || 'guest'}`;
const paperKey = (email?: string) => `spectra-paper-${email || 'guest'}`;
const SOURCES: Source[] = ['engine sample', 'bundled sample', 'uploaded', 'pasted', 'edited'];

export function PortfolioProvider({ children }: { children: ReactNode }) {
  const engine = useEngineHealth();
  const { user } = useAuth();
  const email = user?.email;
  const [positions, setPos] = useState<Position[]>(() => parseCsv(SAMPLE_CSV).positions);
  const [source, setSource] = useState<Source>('bundled sample');
  const [revision, setRevision] = useState(0);
  const [paper, setPaperState] = useState<PaperSettings>(DEFAULT_PAPER);

  /** Which account's draft is currently loaded; persistence waits until it matches. */
  const hydratedFor = useRef<string | null>(null);

  // Load this account's saved book and paper settings. Runs again when the account changes
  // (sign-out, or another user signing in), and always starts from a clean in-memory state
  // so nothing from the previous account survives.
  useEffect(() => {
    hydratedFor.current = null;
    let next = parseCsv(SAMPLE_CSV).positions;
    let nextSource: Source = 'bundled sample';
    let nextPaper: PaperSettings = DEFAULT_PAPER;
    try {
      const raw = localStorage.getItem(portfolioKey(email));
      if (raw) {
        const d = JSON.parse(raw) as { csv?: unknown; source?: unknown };
        const parsed = typeof d.csv === 'string' ? parseCsv(d.csv) : null;
        if (parsed && parsed.positions.length && !parsed.errors.length) {
          next = parsed.positions;
          nextSource = SOURCES.includes(d.source as Source) ? (d.source as Source) : 'edited';
        }
      }
    } catch {
      /* corrupted draft: start from the sample */
    }
    try {
      const raw = localStorage.getItem(paperKey(email));
      if (raw) {
        const p = JSON.parse(raw) as Partial<PaperSettings>;
        if (Number.isFinite(p.startingCapital) && Number.isFinite(p.leverageLimit)) nextPaper = { startingCapital: p.startingCapital!, leverageLimit: p.leverageLimit! };
      }
    } catch {
      /* defaults */
    }
    setPos(next);
    setSource(nextSource);
    setPaperState(nextPaper);
    setRevision((r) => r + 1);
    hydratedFor.current = portfolioKey(email);
  }, [email]);

  // Save the draft under the current account only (never during the account switch itself).
  useEffect(() => {
    const key = portfolioKey(email);
    if (hydratedFor.current !== key) return;
    try {
      localStorage.setItem(key, JSON.stringify({ csv: toCsv(positions), source }));
    } catch {
      /* not persisted */
    }
  }, [positions, source, email]);

  const setPaper = useCallback(
    (p: PaperSettings) => {
      setPaperState(p);
      try {
        localStorage.setItem(paperKey(email), JSON.stringify(p));
      } catch {
        /* not persisted */
      }
    },
    [email],
  );

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

  const value = useMemo<PortfolioCtx>(() => {
    const stats = portfolioStats(positions);
    return { positions, csv: toCsv(positions), stats, source, revision, engine, paper, account: paperAccount(stats, paper), setPaper, setPositions, loadSample, importCsv };
  }, [positions, source, revision, engine, paper, setPaper, setPositions, loadSample, importCsv]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function usePortfolio() {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error('usePortfolio must be used inside <PortfolioProvider>');
  return ctx;
}
