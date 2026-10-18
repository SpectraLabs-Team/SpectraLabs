import type { EngineScenario } from './types';

export interface Scenario {
  id: string;
  name: string;
  description: string;
  /** Every price is multiplied by this. */
  priceFactor: number;
  /** Every asset volatility is multiplied by this after the shock (1 = unchanged). */
  volMultiplier: number;
  /**
   * `engine`: a built-in /api/stress scenario.
   * `custom`: the shocked CSV is built here and sent to /api/risk, so post-shock VaR is still an
   * engine Monte Carlo result.
   */
  source: { kind: 'engine'; scenario: EngineScenario } | { kind: 'custom' };
}

// Custom scenarios are hypothetical uniform shocks loosely sized after historical episodes,
// not historical replays: every position moves by the same percentage.
export const SCENARIOS: Scenario[] = [
  {
    id: 'market-crash',
    name: 'Market crash',
    description: 'Engine built-in: every price falls 30%.',
    priceFactor: 0.7,
    volMultiplier: 1,
    source: { kind: 'engine', scenario: 'market-crash' },
  },
  {
    id: 'mild-correction',
    name: 'Mild correction',
    description: 'Engine built-in: every price falls 10%.',
    priceFactor: 0.9,
    volMultiplier: 1,
    source: { kind: 'engine', scenario: 'mild-correction' },
  },
  {
    id: 'covid',
    name: 'COVID-style crash',
    description: 'Prices −30%, volatility ×2 (Feb–Mar 2020 sized).',
    priceFactor: 0.7,
    volMultiplier: 2,
    source: { kind: 'custom' },
  },
  {
    id: 'lehman',
    name: 'Lehman-style crisis',
    description: 'Prices −40%, volatility ×2.5 (2008 sized).',
    priceFactor: 0.6,
    volMultiplier: 2.5,
    source: { kind: 'custom' },
  },
  {
    id: 'tech-selloff',
    name: 'Tech selloff',
    description: 'Prices −20%, volatility ×1.5.',
    priceFactor: 0.8,
    volMultiplier: 1.5,
    source: { kind: 'custom' },
  },
  {
    id: 'rates-spike',
    name: 'Rates spike',
    description: 'Prices −12%, volatility ×1.3 (duration-led repricing).',
    priceFactor: 0.88,
    volMultiplier: 1.3,
    source: { kind: 'custom' },
  },
];

/** /api/stress always reports post-shock VaR at 99%; custom scenarios use the same level for comparability. */
export const STRESS_CONFIDENCE = 0.99;
