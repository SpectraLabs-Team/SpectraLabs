'use client';

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { useAuth } from '@/lib/auth';
import { getSamplePortfolio } from '@/lib/api';
import { DEFAULT_PAPER, paperAccount, type PaperAccount, type PaperSettings } from '@/lib/paperLedger';
import { SAMPLE_CSV, parseCsv, portfolioStats, toCsv, type PortfolioStats } from '@/lib/portfolio';
import { PRESETS } from '@/lib/presets';
import type { BackendRequest, Position, RiskResult } from '@/lib/types';
import { useEngineHealth, type EngineState } from '@/lib/useEngineHealth';

type Source = 'engine sample' | 'bundled sample' | 'uploaded' | 'pasted' | 'edited' | 'preset';

/** Risk Lab inputs. Kept here (not in the tab) so switching Studio tabs doesn't reset them. */
export interface RiskLabParams {
  confidence: number;
  simulations: number;
  horizon: number;
  auto: boolean;
}

/** The last Risk Lab run plus the exact inputs it was made with (for the freshness check). */
export interface RiskRun {
  var: RiskResult;
  cvar: RiskResult;
  vol: RiskResult;
  /** Portfolio revision the run was made against. */
  rev: number;
  simulations: number;
  backend: BackendRequest;
  /** When the run finished (ms epoch), for the exported report. */
  at: number;
  /** The exact book the run used, so an export never mixes old results with a newer book. */
  csv: string;
}

export interface StressOutcome {
  valueAfter: number;
  immediateLoss: number;
  postVaR: number;
  backend: string;
  computeMs: number;
}

/** Completed stress scenarios. `key` = the book + settings they were run with; a mismatch means stale. */
export interface StressCache {
  key: string;
  results: Record<string, StressOutcome>;
  baseVaR: number | null;
}

export const DEFAULT_RISK_PARAMS: RiskLabParams = { confidence: 0.99, simulations: 100_000, horizon: 1, auto: false };
const EMPTY_STRESS: StressCache = { key: '', results: {}, baseVaR: null };

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
  loadPreset: (id: string) => void;
  riskParams: RiskLabParams;
  setRiskParams: (update: Partial<RiskLabParams> | ((p: RiskLabParams) => Partial<RiskLabParams>)) => void;
  riskRun: RiskRun | null;
  setRiskRun: (r: RiskRun | null) => void;
  stress: StressCache;
  setStress: (update: (s: StressCache) => StressCache) => void;
}

const Ctx = createContext<PortfolioCtx | null>(null);
/** Per-account storage keys, so two people on one browser never see each other's book. */
const portfolioKey = (email?: string) => `spectra-portfolio-${email || 'guest'}`;
const paperKey = (email?: string) => `spectra-paper-${email || 'guest'}`;
const SOURCES: Source[] = ['engine sample', 'bundled sample', 'uploaded', 'pasted', 'edited', 'preset'];

export function PortfolioProvider({ children }: { children: ReactNode }) {
  const engine = useEngineHealth();
  const { user } = useAuth();
  const email = user?.email;
  const [positions, setPos] = useState<Position[]>(() => parseCsv(SAMPLE_CSV).positions);
  const [source, setSource] = useState<Source>('bundled sample');
  const [revision, setRevision] = useState(0);
  const [paper, setPaperState] = useState<PaperSettings>(DEFAULT_PAPER);
  const [riskParams, setRiskParamsState] = useState<RiskLabParams>(DEFAULT_RISK_PARAMS);
  const [riskRun, setRiskRun] = useState<RiskRun | null>(null);
  const [stress, setStressState] = useState<StressCache>(EMPTY_STRESS);

  /**
   * Which account's draft is loaded. It is state (not a ref) on purpose: it updates in the same
   * render as the restored positions, so the save effect can never pair one account's key with
   * another account's (or the default sample's) positions.
   */
  const [loadedFor, setLoadedFor] = useState<string | null>(null);

  // Load this account's saved book and paper settings. Runs again when the account changes
  // (sign-out, or another user signing in), and always starts from a clean in-memory state
  // so nothing from the previous account survives.
  useEffect(() => {
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
    // Session results belong to the previous account: never carry them across a switch.
    setRiskParamsState(DEFAULT_RISK_PARAMS);
    setRiskRun(null);
    setStressState(EMPTY_STRESS);
    setRevision((r) => r + 1);
    setLoadedFor(portfolioKey(email));
  }, [email]);

  // Save the draft under the current account only (never during the account switch itself).
  useEffect(() => {
    const key = portfolioKey(email);
    if (loadedFor !== key) return;
    try {
      localStorage.setItem(key, JSON.stringify({ csv: toCsv(positions), source }));
    } catch {
      /* not persisted */
    }
  }, [positions, source, email, loadedFor]);

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

  const loadPreset = useCallback(
    (id: string) => {
      const preset = PRESETS.find((p) => p.id === id);
      if (preset) setPositions(parseCsv(preset.csv).positions, 'preset');
    },
    [setPositions],
  );

  const setRiskParams = useCallback((update: Partial<RiskLabParams> | ((p: RiskLabParams) => Partial<RiskLabParams>)) => {
    setRiskParamsState((p) => ({ ...p, ...(typeof update === 'function' ? update(p) : update) }));
  }, []);
  const setStress = useCallback((update: (s: StressCache) => StressCache) => setStressState(update), []);

  const value = useMemo<PortfolioCtx>(() => {
    const stats = portfolioStats(positions);
    return {
      positions, csv: toCsv(positions), stats, source, revision, engine, paper, account: paperAccount(stats, paper),
      setPaper, setPositions, loadSample, importCsv, loadPreset,
      riskParams, setRiskParams, riskRun, setRiskRun, stress, setStress,
    };
  }, [positions, source, revision, engine, paper, setPaper, setPositions, loadSample, importCsv, loadPreset, riskParams, setRiskParams, riskRun, stress, setStress]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function usePortfolio() {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error('usePortfolio must be used inside <PortfolioProvider>');
  return ctx;
}
