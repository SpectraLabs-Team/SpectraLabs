'use client';

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useAuth } from '@/lib/auth';
import { getSamplePortfolio } from '@/lib/api';
import { DEFAULT_PAPER, paperAccount, type PaperAccount, type PaperSettings } from '@/lib/paperLedger';
import { SAMPLE_CSV, parseCsv, portfolioStats, toCsv, type PortfolioStats } from '@/lib/portfolio';
import { PRESETS } from '@/lib/presets';
import type { BackendRequest, Position, RiskResult } from '@/lib/types';
import { useEngineHealth, type EngineState } from '@/lib/useEngineHealth';
import { useToast } from '@/components/ui/StudioToast';
import {
  ensureImported,
  getUserState,
  readCache,
  StateSaver,
  writeCache,
  type PortfolioValue,
  type StateEntry,
} from '@/lib/userState';

type Source = 'engine sample' | 'bundled sample' | 'uploaded' | 'pasted' | 'edited' | 'preset';

/** Risk Lab inputs. Kept here (not in the tab) so switching Studio tabs doesn't reset them. */
export interface RiskLabParams {
  confidence: number;
  simulations: number;
  horizon: number;
  auto: boolean;
  dependence?: 'aggregate' | 'gaussian_copula' | 't_copula';
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
  loadSample: (notify?: boolean) => Promise<void>;
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
const SOURCES: Source[] = ['engine sample', 'bundled sample', 'uploaded', 'pasted', 'edited', 'preset'];

/** A saved book parsed back into positions, or null when it is missing or unusable. */
function bookFrom(v: PortfolioValue | null | undefined): { positions: Position[]; source: Source } | null {
  if (!v || typeof v.csv !== 'string') return null;
  const parsed = parseCsv(v.csv);
  if (!parsed.positions.length || parsed.errors.length) return null;
  return {
    positions: parsed.positions,
    source: SOURCES.includes(v.source as Source) ? (v.source as Source) : 'edited',
  };
}

function paperFrom(v: unknown): PaperSettings | null {
  const p = v as Partial<PaperSettings> | null;
  if (!p || !Number.isFinite(p.startingCapital) || !Number.isFinite(p.leverageLimit)) return null;
  return { startingCapital: p.startingCapital!, leverageLimit: p.leverageLimit! };
}

function riskFrom(v: unknown): RiskLabParams | null {
  const rp = v as Partial<RiskLabParams> | null;
  if (!rp || typeof rp.confidence !== 'number' || !Number.isFinite(rp.confidence)) return null;
  return {
    confidence: Math.min(0.999, Math.max(0.9, rp.confidence)),
    simulations: typeof rp.simulations === 'number' && rp.simulations > 0 ? rp.simulations : DEFAULT_RISK_PARAMS.simulations,
    horizon: typeof rp.horizon === 'number' && rp.horizon > 0 ? rp.horizon : DEFAULT_RISK_PARAMS.horizon,
    auto: Boolean(rp.auto),
    dependence: rp.dependence === 'gaussian_copula' || rp.dependence === 't_copula' ? rp.dependence : 'aggregate',
  };
}

const bookValue = (positions: Position[], source: Source): PortfolioValue => ({ csv: toCsv(positions), source });

interface Savers {
  portfolio: StateSaver<PortfolioValue>;
  paper: StateSaver<PaperSettings>;
  risk: StateSaver<RiskLabParams>;
}

export function PortfolioProvider({ children }: { children: ReactNode }) {
  const engine = useEngineHealth();
  const { user } = useAuth();
  const email = user?.email;
  /** Browser-cache key owner. Guests keep a local-only copy and never reach the server. */
  const who = email || 'guest';
  const { success, info } = useToast();
  const [positions, setPos] = useState<Position[]>(() => parseCsv(SAMPLE_CSV).positions);
  const [source, setSource] = useState<Source>('bundled sample');
  const [revision, setRevision] = useState(0);
  const [paper, setPaperState] = useState<PaperSettings>(DEFAULT_PAPER);
  const [riskParams, setRiskParamsState] = useState<RiskLabParams>(DEFAULT_RISK_PARAMS);
  const [riskRun, setRiskRun] = useState<RiskRun | null>(null);
  const [stress, setStressState] = useState<StressCache>(EMPTY_STRESS);

  /**
   * Which account's cache is on screen. State (not a ref) so it updates in the same render as the
   * restored values: a save can never pair one account's key with another account's data.
   */
  const [loadedFor, setLoadedFor] = useState<string | null>(null);
  /** Which account's server copy has been applied. Nothing is written to the server before this. */
  const [syncedFor, setSyncedFor] = useState<string | null>(null);
  const savers = useRef<Savers | null>(null);

  // Paint from the browser cache at once, then replace it with the server copy. Runs again when the
  // account changes, so nothing from the previous account survives.
  useEffect(() => {
    let alive = true;
    const id = who;

    const cachedBook = bookFrom(readCache<PortfolioValue>(id, 'portfolio'));
    const cachedPaper = paperFrom(readCache(id, 'paper-account'));
    const cachedRisk = riskFrom(readCache(id, 'risk-params'));
    const paintedPositions = cachedBook?.positions ?? parseCsv(SAMPLE_CSV).positions;
    const paintedSource: Source = cachedBook?.source ?? 'bundled sample';
    setPos(paintedPositions);
    setSource(paintedSource);
    setPaperState(cachedPaper ?? DEFAULT_PAPER);
    setRiskParamsState(cachedRisk ?? DEFAULT_RISK_PARAMS);
    setRiskRun(null);
    setStressState(EMPTY_STRESS);
    setRevision((r) => r + 1);
    setLoadedFor(id);
    setSyncedFor(null);

    const s: Savers = {
      portfolio: new StateSaver<PortfolioValue>('portfolio', (cur) => {
        const b = bookFrom(cur.value);
        if (b) {
          setPos(b.positions);
          setSource(b.source);
          setRevision((r) => r + 1);
        }
        info('Portfolio updated', 'Changed on another device; loaded the latest copy.');
      }),
      paper: new StateSaver<PaperSettings>('paper-account', (cur) => {
        setPaperState(paperFrom(cur.value) ?? DEFAULT_PAPER);
      }),
      risk: new StateSaver<RiskLabParams>('risk-params', (cur) => {
        setRiskParamsState(riskFrom(cur.value) ?? DEFAULT_RISK_PARAMS);
      }),
    };
    savers.current = s;

    if (!email) {
      return () => {
        alive = false;
        s.portfolio.dispose();
        s.paper.dispose();
        s.risk.dispose();
      };
    }

    (async () => {
      try {
        await ensureImported(id);
        const st = await getUserState();
        if (!alive) return;

        const p = st.state.portfolio as StateEntry<PortfolioValue>;
        if (p.version > 0) {
          const b = bookFrom(p.value);
          if (b) {
            setPos(b.positions);
            setSource(b.source);
            setRevision((r) => r + 1);
            s.portfolio.baseline(p.version, bookValue(b.positions, b.source));
          } else {
            s.portfolio.baseline(p.version, p.value);
          }
        } else {
          // Nothing saved for this account yet: push what is on screen so every device starts from it.
          s.portfolio.baseline(0, null);
          s.portfolio.schedule(bookValue(paintedPositions, paintedSource), 0);
        }

        const pa = st.state['paper-account'] as StateEntry<unknown>;
        if (pa.version > 0) {
          const settings = paperFrom(pa.value);
          if (settings) setPaperState(settings);
          s.paper.baseline(pa.version, settings ?? DEFAULT_PAPER);
        } else {
          s.paper.baseline(0, null);
          s.paper.schedule(cachedPaper ?? DEFAULT_PAPER, 0);
        }

        const rk = st.state['risk-params'] as StateEntry<unknown>;
        if (rk.version > 0) {
          const risk = riskFrom(rk.value);
          if (risk) setRiskParamsState(risk);
          s.risk.baseline(rk.version, risk ?? DEFAULT_RISK_PARAMS);
        } else {
          s.risk.baseline(0, null);
          s.risk.schedule(cachedRisk ?? DEFAULT_RISK_PARAMS, 0);
        }

        setSyncedFor(id);
      } catch {
        // Could not reach the server: keep the browser copy on screen. Nothing is written to the server
        // until a load succeeds, so an old copy cannot overwrite newer data.
        if (alive) info('Saved data unavailable', 'Showing the copy on this browser. Changes will sync when you reconnect.');
      }
    })();

    return () => {
      alive = false;
      s.portfolio.dispose();
      s.paper.dispose();
      s.risk.dispose();
    };
    // `info` is a stable toast callback; only the account matters here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [who, email]);

  // Keep the cache current, and save to the server once this account is synced. The cache is gated on
  // `loadedFor` so the default sample can never overwrite an account's cached book on mount.
  useEffect(() => {
    if (loadedFor !== who) return;
    const book = bookValue(positions, source);
    writeCache(who, 'portfolio', book);
    if (syncedFor === who) savers.current?.portfolio.schedule(book);
  }, [positions, source, who, loadedFor, syncedFor]);

  const setPaper = useCallback(
    (p: PaperSettings) => {
      setPaperState(p);
      writeCache(who, 'paper-account', p);
      if (syncedFor === who) savers.current?.paper.schedule(p);
    },
    [who, syncedFor],
  );

  const setPositions = useCallback((p: Position[], s: Source) => {
    setPos(p);
    setSource(s);
    setRevision((r) => r + 1);
  }, []);

  const loadSample = useCallback(async (notify = false) => {
    try {
      setPositions(parseCsv(await getSamplePortfolio()).positions, 'engine sample');
      if (notify) info('Sample portfolio loaded', 'Synchronized from live engine sample');
    } catch {
      setPositions(parseCsv(SAMPLE_CSV).positions, 'bundled sample');
      if (notify) info('Sample portfolio loaded', 'Loaded bundled benchmark portfolio');
    }
  }, [setPositions, info]);

  // Swap in the engine's sample once it is reachable, unless the user already changed the book.
  const online = engine.status === 'online';
  useEffect(() => {
    if (online && source === 'bundled sample') loadSample(false);
  }, [online, source, loadSample]);

  const importCsv = useCallback(
    (text: string, s: 'uploaded' | 'pasted') => {
      const { positions: p, errors } = parseCsv(text);
      if (!errors.length && p.length) {
        setPositions(p, s);
        success(`Imported ${p.length} positions`, `Successfully parsed and loaded via ${s} CSV`);
      } else if (!p.length && !errors.length) {
        return ['No positions found. Expected a header row, then symbol,quantity,price[,volatility].'];
      }
      return errors;
    },
    [setPositions, success],
  );

  const loadPreset = useCallback(
    (id: string) => {
      const preset = PRESETS.find((p) => p.id === id);
      if (preset) {
        const parsed = parseCsv(preset.csv);
        setPositions(parsed.positions, 'preset');
        success(`Loaded preset: ${preset.name}`, `${parsed.positions.length} positions staged in active book`);
      }
    },
    [setPositions, success],
  );

  const setRiskParams = useCallback(
    (update: Partial<RiskLabParams> | ((p: RiskLabParams) => Partial<RiskLabParams>)) => {
      setRiskParamsState((prev) => {
        const next = { ...prev, ...(typeof update === 'function' ? update(prev) : update) };
        writeCache(who, 'risk-params', next);
        if (syncedFor === who) savers.current?.risk.schedule(next);
        return next;
      });
    },
    [who, syncedFor],
  );
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
