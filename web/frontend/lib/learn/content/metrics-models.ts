import { metricLesson } from '../build';

const RISK = [{ label: 'Open Risk & Stress Lab', href: '/studio?tab=risk' }];

export const modelMetrics = [
  metricLesson({
    slug: 'engine-benchmark',
    title: 'Engine Benchmark: Latency, Throughput and Speed-up',
    level: 'Intermediate',
    summary: 'How fast the risk engine runs a Monte Carlo VaR calculation on this machine, measured as median latency, paths per second and the parallel speed-up.',
    keywords: ['benchmark', 'latency', 'p50', 'median', 'throughput', 'paths per second', 'speed-up', 'parallel', 'performance'],
    what: [
      'The Benchmark Lab times the risk engine running a full **99% Monte Carlo VaR** calculation on the sample portfolio, at several path counts and on two backends: **CPU** (single-threaded reference) and **Parallel** (all cores). Each size runs a few warm-up passes that are discarded, then a number of timed repetitions.',
      'From the timed runs it reports the **median latency (p50)**, the **throughput** in paths per second, and the **speed-up** of Parallel over CPU.',
    ],
    why: [
      'Latency tells you how long you wait for a risk number; throughput tells you how many paths you can afford for a given wait. More paths give a more stable VaR estimate, so faster runs buy accuracy.',
    ],
    formulas: [
      { tex: String.raw`\text{latency}_{p50} = \operatorname{median}(t_1, \dots, t_R)`, caption: 'R timed repetitions, warm-ups excluded' },
      { tex: String.raw`\text{throughput} = \frac{N_{\text{paths}} \times 1000}{\text{latency}_{p50}\ (\text{ms})}` },
      { tex: String.raw`\text{speed-up} = \frac{\text{latency}_{p50}(\text{CPU})}{\text{latency}_{p50}(\text{Parallel})}` },
    ],
    where: ['\(t_i\) is the wall-clock time of one complete VaR run in milliseconds.'],
    engine:
      '`spectra_risk` `benchmark`: for each backend it runs the warm-ups, then the repetitions, sorts the run times and reports min, mean, p50, p95, p99 and max, plus `paths_per_second` computed from the median. If a backend is unavailable the engine falls back and marks the row as a fallback.',
    example: {
      intro: 'At 1,000,000 paths the CPU backend has a median of 400 ms and the Parallel backend a median of 80 ms.',
      steps: ['CPU throughput: 1,000,000 × 1000 / 400 = 2.5 million paths per second.', 'Parallel throughput: 1,000,000 × 1000 / 80 = 12.5 million paths per second.', 'Speed-up: 400 / 80 = 5.0×.'],
      result: 'Parallel is **5× faster**, running **12.5 million paths per second**.',
    },
    high: 'High throughput and a speed-up well above 1×: large simulations are cheap on this machine.',
    low: 'A speed-up below 1× at small path counts is normal: starting threads costs more than the work saved.',
    misconceptions: [
      'These figures depend on the hardware and on what else the machine is doing; compare runs on the same host only.',
      'A faster engine does not make VaR more accurate by itself; it lets you afford more paths.',
    ],
    related: ['simulations/monte-carlo-risk-simulation'],
    platformLinks: [{ label: 'Open Benchmark Lab', href: '/studio?tab=benchmark' }],
  }),
  metricLesson({
    slug: 'garch-volatility',
    title: 'GARCH Volatility Forecast',
    level: 'Advanced',
    summary: 'A volatility model in which today’s variance depends on yesterday’s shock and yesterday’s variance, so calm and turbulent periods persist.',
    keywords: ['garch', 'gjr', 'egarch', 'volatility forecast', 'persistence', 'leverage effect', 'conditional volatility'],
    what: [
      'GARCH models capture **volatility clustering**: large moves tend to follow large moves. The Risk & Stress Lab fits GARCH(1,1), GJR-GARCH(1,1) or EGARCH(1,1) to returns and reports a one-day-ahead volatility forecast (annualized), the long-run (unconditional) volatility, the persistence and, for GJR, the asymmetric leverage coefficient γ.',
    ],
    why: [
      'A constant volatility understates risk after a shock and overstates it in calm periods. A conditional forecast gives VaR and position sizes that react to the current market.',
    ],
    formulas: [
      { tex: String.raw`\sigma_t^2 = \omega + \alpha\,\epsilon_{t-1}^2 + \beta\,\sigma_{t-1}^2`, caption: 'GARCH(1,1)' },
      { tex: String.raw`\sigma_t^2 = \omega + \left(\alpha + \gamma\,\mathbf{1}_{\{\epsilon_{t-1} < 0\}}\right)\epsilon_{t-1}^2 + \beta\,\sigma_{t-1}^2`, caption: 'GJR-GARCH(1,1): negative shocks add γ' },
      { tex: String.raw`\sigma_L^2 = \frac{\omega}{1 - \alpha - \beta}, \qquad \text{persistence} = \alpha + \beta \ \left(+\ \gamma/2 \text{ for GJR}\right)` },
    ],
    where: ['\\(\\epsilon_{t-1}\\) is yesterday’s return minus its mean.', 'Annualized values multiply the daily standard deviation by √252.'],
    engine:
      'Fitted in `spectra_risk::garch` by maximum likelihood with **variance targeting**: ω is set so the long-run variance equals the sample variance, \\(\\omega = \\sigma_L^2 (1 - \\alpha - \\beta)\\). Fits need at least 30 returns and a non-zero variance.',
    example: {
      intro: 'A fit gives α = 0.08, β = 0.90 and a long-run daily variance of 0.0001 (1% a day). Yesterday’s shock was −3% and yesterday’s variance was 0.0002.',
      steps: [
        'ω = 0.0001 × (1 − 0.08 − 0.90) = 0.000002.',
        'Tomorrow: 0.000002 + 0.08 × 0.0009 + 0.90 × 0.0002 = 0.000254.',
        'Daily volatility √0.000254 = 1.59%; annualized ≈ 25.3%, against a long-run 15.9%.',
      ],
      result: 'The forecast is about **25% annualized**, well above the long-run **15.9%**, and decays back towards it at the rate set by persistence (0.98 a day).',
    },
    high: 'A forecast above the long-run level: risk is elevated now. Persistence near 1: shocks take a long time to fade. γ > 0: falls raise volatility more than rises.',
    low: 'A forecast below the long-run level: a calm period. Low persistence: shocks fade quickly.',
    misconceptions: ['GARCH forecasts volatility, not direction.', 'Persistence at or above 1 means the model has no stable long-run level; treat such fits with care.'],
    related: ['volatility', 'value-at-risk', 'market-regimes', 'fundamentals/volatility'],
    platformLinks: RISK,
  }),

  metricLesson({
    slug: 'market-regimes',
    title: 'Market Regimes & Regime-Conditional VaR',
    level: 'Advanced',
    summary: 'A hidden Markov model splits history into calm and turbulent regimes, estimates how likely the market is to switch, and computes VaR for each regime.',
    keywords: ['regime', 'hmm', 'hidden markov model', 'transition matrix', 'regime var', 'jump model', 'bull bear'],
    what: [
      'A Hidden Markov Model (HMM) assumes returns come from one of a few hidden states, each with its own mean and volatility (for example a low-volatility and a high-volatility state). The Risk & Stress Lab fits it to returns, reports the **detected current regime**, the **transition probability matrix** and a **regime-conditional VaR**.',
    ],
    why: ['Risk is not constant. A VaR computed with calm-period volatility badly understates losses in a crisis; knowing the current regime and the chance of switching makes risk estimates more honest.'],
    formulas: [
      { tex: String.raw`P_{ij} = \Pr(S_t = j \mid S_{t-1} = i)`, caption: 'Transition matrix: each row sums to 1' },
      { tex: String.raw`\text{VaR}_k = V\left(1 - \exp\!\left(\left(\mu_k\,252 - \tfrac{\sigma_k^2}{2}\right)T - \sigma_k\sqrt{T}\,z_c\right)\right)`, caption: 'VaR in regime k, with its own drift and volatility' },
    ],
    where: ['\\(\\mu_k\\) is the regime’s daily mean return, \\(\\sigma_k\\) its annualized volatility, \\(z_c\\) the normal quantile of the confidence.'],
    engine:
      '`spectra_risk::regime` fits two- or three-state Gaussian HMMs with Baum–Welch (EM), decodes the most likely path with Viterbi, and classifies the current regime from the filtered probability at the end of the sample. A statistical jump model with a penalty against frequent switching is also available.',
    example: {
      intro: 'A two-state fit has a calm regime with 12% volatility and a turbulent one with 35%. The transition matrix has P(calm → calm) = 0.98 and P(turbulent → turbulent) = 0.93.',
      steps: ['Expected stay in calm: 1 / (1 − 0.98) = 50 days.', 'Expected stay in turbulence: 1 / (1 − 0.93) ≈ 14 days.', 'The one-day 95% VaR in the turbulent regime is roughly three times the calm one (35% vs 12% volatility).'],
      result: 'If the current regime is turbulent, the **regime-conditional VaR** is the number to plan with, not the calm-period one.',
    },
    high: 'High diagonal probabilities: regimes are persistent. A high-volatility current regime: expect larger moves.',
    low: 'Low diagonal probabilities: the market switches often and regime labels are less useful.',
    misconceptions: ['Regimes are labels the model finds in the data; they are not announced, and the current one is only known with some probability.'],
    related: ['value-at-risk', 'garch-volatility', 'volatility'],
    platformLinks: RISK,
  }),
];
