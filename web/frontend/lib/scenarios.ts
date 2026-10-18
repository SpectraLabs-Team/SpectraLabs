import type { EngineScenario } from './types';

export interface Scenario {
  id: string;
  name: string;
  /** Short shock label for cards, e.g. "−35%". */
  shock: string;
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

// Custom scenarios are hypothetical uniform shocks sized after historical episodes,
// not historical replays: every position moves by the same percentage.
export const SCENARIOS: Scenario[] = [
  { id: 'covid', name: 'COVID crash', shock: '−35%', description: 'Prices −35%, volatility ×2. Sized after Feb–Mar 2020.', priceFactor: 0.65, volMultiplier: 2, source: { kind: 'custom' } },
  { id: 'lehman', name: '2008 Lehman', shock: '−45%', description: 'Prices −45%, volatility ×2.5. Sized after Sep 2008–Mar 2009.', priceFactor: 0.55, volMultiplier: 2.5, source: { kind: 'custom' } },
  { id: 'tech-selloff', name: 'Tech selloff', shock: '−20%', description: 'Prices −20%, volatility ×1.5.', priceFactor: 0.8, volMultiplier: 1.5, source: { kind: 'custom' } },
  { id: 'market-crash', name: 'Market crash', shock: '−30%', description: 'Engine built-in: every price falls 30%.', priceFactor: 0.7, volMultiplier: 1, source: { kind: 'engine', scenario: 'market-crash' } },
  { id: 'mild-correction', name: 'Mild correction', shock: '−10%', description: 'Engine built-in: every price falls 10%.', priceFactor: 0.9, volMultiplier: 1, source: { kind: 'engine', scenario: 'mild-correction' } },
  { id: 'rates-spike', name: 'Rates spike', shock: '−12%', description: 'Prices −12%, volatility ×1.3. Duration-led repricing.', priceFactor: 0.88, volMultiplier: 1.3, source: { kind: 'custom' } },
];

/** /api/stress always reports post-shock VaR at 99%; custom scenarios use the same level for comparability. */
export const STRESS_CONFIDENCE = 0.99;
