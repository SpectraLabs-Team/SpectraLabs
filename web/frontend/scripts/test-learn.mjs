// Learning Center, metric tooltips and guided tour: content integrity, links, formulas, search and
// tour logic. No database or browser needed.
// Run: node scripts/test-learn.mjs
import { describe, it, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);
const ts = require(path.join(root, 'node_modules/typescript'));

// Transpile the modules under test to CommonJS, keeping the directory layout so relative imports resolve.
const build = path.join(root, 'node_modules', '.cache', 'spectra-learn-test');
fs.rmSync(build, { recursive: true, force: true });
fs.mkdirSync(build, { recursive: true });
fs.writeFileSync(path.join(build, 'package.json'), '{"type":"commonjs"}');
const walk = (dir) => fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(dir, e.name)) : [path.join(dir, e.name)]));
const sources = [...walk(path.join(root, 'lib/learn')), path.join(root, 'lib/onboarding.ts'), path.join(root, 'components/onboarding/tourSteps.ts')].filter((f) => f.endsWith('.ts'));
for (const file of sources) {
  const { outputText } = ts.transpileModule(fs.readFileSync(file, 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, esModuleInterop: true } });
  const out = path.join(build, path.relative(root, file).replace(/\.ts$/, '.js'));
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, outputText);
}
after(() => fs.rmSync(build, { recursive: true, force: true }));
const load = (rel) => require(path.join(build, rel));

const learn = load('lib/learn/index.js');
const { metricDefinitions } = load('lib/learn/metrics.js');
const calc = load('lib/learn/calc.js');
const { searchLessons } = load('lib/learn/search.js');
const onboarding = load('lib/onboarding.js');
const { MAIN_TOUR, PAGE_TOURS } = load('components/onboarding/tourSteps.js');

const src = (rel) => fs.readFileSync(path.join(root, rel), 'utf8');
const shell = src('components/studio/StudioShell.tsx');
const STUDIO_TABS = [...shell.matchAll(/\{ id: '([a-z-]+)', label:/g)].map((m) => m[1]);
const { LESSONS, CATEGORIES } = learn;
const key = (l) => `${l.category}/${l.slug}`;

/** Every string in a lesson (for markup and control-character checks). */
function strings(l) {
  const out = [l.title, l.summary, ...(l.keywords ?? [])];
  for (const b of l.blocks) {
    if ('text' in b) out.push(b.text);
    if ('items' in b) out.push(...b.items);
    if ('head' in b) out.push(...b.head, ...b.rows.flat());
    if ('caption' in b && b.caption) out.push(b.caption);
    if ('title' in b && b.title) out.push(b.title);
  }
  return out;
}

/** An internal href points at something that exists. */
function resolves(href) {
  const [p, q = ''] = href.split('?');
  if (p === '/learn') return true;
  if (p.startsWith('/learn/')) {
    const parts = p.split('/').filter(Boolean);
    if (parts.length === 2) return CATEGORIES.some((c) => c.id === parts[1]);
    if (parts.length === 3) return !!learn.getLesson(parts[1], parts[2]);
    return false;
  }
  if (p === '/studio') {
    const tab = new URLSearchParams(q).get('tab');
    return !tab || STUDIO_TABS.includes(tab);
  }
  if (p === '/architecture') return fs.existsSync(path.join(root, 'app/architecture/page.tsx'));
  return false;
}

describe('Learning Center content', () => {
  it('has the six categories, each with lessons, and unique lesson routes', () => {
    assert.deepEqual(CATEGORIES.map((c) => c.id), ['fundamentals', 'engine', 'simulations', 'metrics', 'strategies', 'paper-trading']);
    for (const c of CATEGORIES) assert.ok(learn.lessonsIn(c.id).length >= 6, `${c.id} has lessons`);
    const keys = LESSONS.map(key);
    assert.equal(new Set(keys).size, keys.length, 'no duplicate routes');
    for (const l of LESSONS) assert.match(l.slug, /^[a-z0-9]+(-[a-z0-9]+)*$/, `${key(l)} slug`);
  });

  it('covers every requested topic', () => {
    const need = {
      fundamentals: ['what-is-trading', 'how-stock-markets-work', 'market-and-limit-orders', 'buy-and-sell-orders', 'bid-and-ask', 'bid-ask-spread', 'liquidity', 'volatility', 'slippage', 'market-depth', 'order-execution', 'trading-sessions'],
      engine: ['what-is-a-trading-engine', 'order-lifecycle', 'order-matching', 'order-book-fundamentals', 'order-execution-engine', 'trade-management', 'risk-management', 'position-management', 'portfolio-management', 'market-data-processing'],
      simulations: ['what-is-a-trading-simulation', 'why-simulations-are-useful', 'how-simulated-trades-execute', 'historical-vs-real-time-simulations', 'simulation-assumptions', 'what-happens-when-a-simulation-runs', 'interpreting-simulation-output', 'simulation-vs-backtesting-vs-paper-trading'],
      metrics: ['total-return', 'net-profit', 'profit-and-loss', 'realized-unrealized-pnl', 'win-rate', 'sharpe-ratio', 'sortino-ratio', 'maximum-drawdown', 'volatility', 'profit-factor', 'average-trade-return', 'risk-reward-ratio', 'expectancy', 'equity-curve', 'annualized-return', 'cagr', 'exposure', 'number-of-trades', 'transaction-costs', 'slippage'],
      strategies: ['moving-average-crossover', 'momentum', 'mean-reversion', 'breakout-trading', 'trend-following', 'rsi-strategies'],
      'paper-trading': ['what-is-paper-trading', 'how-alpaca-paper-trading-works', 'connecting-alpaca', 'searching-for-stocks', 'reading-candlestick-charts', 'placing-orders', 'order-statuses', 'monitoring-positions', 'tracking-portfolio-performance'],
    };
    for (const [cat, slugs] of Object.entries(need)) for (const s of slugs) assert.ok(learn.getLesson(cat, s), `missing ${cat}/${s}`);
  });

  it('is real content: every lesson has a summary, several blocks and a reading time', () => {
    for (const l of LESSONS) {
      assert.ok(l.summary.length >= 40, `${key(l)} summary`);
      assert.ok(l.blocks.length >= 2, `${key(l)} has content`);
      assert.ok(['Beginner', 'Intermediate', 'Advanced'].includes(l.level), `${key(l)} level`);
      const words = learn.lessonText(l).split(/\s+/).length;
      if (process.env.LIST_SHORT && words < 160) console.log("SHORT", key(l), words);
      else assert.ok(words >= 120, `${key(l)} is not a placeholder (${words} words)`);
      assert.ok(learn.readingMinutes(l) >= 1);
    }
  });

  it('metric lessons have all eight parts', () => {
    for (const l of learn.lessonsIn('metrics')) {
      const heads = l.blocks.filter((b) => b.type === 'h').map((b) => b.text);
      for (const h of ['What it measures', 'Why it matters', 'Worked example', 'Reading high and low values', 'Common misconceptions']) assert.ok(heads.includes(h), `${key(l)}: ${h}`);
      assert.ok(l.blocks.some((b) => b.type === 'formula'), `${key(l)}: formula`);
      assert.ok(l.blocks.some((b) => b.type === 'callout' && b.title === 'How Spectra calculates it'), `${key(l)}: engine note`);
      assert.ok(l.related.length >= 1, `${key(l)}: related metrics`);
    }
  });

  it('every related lesson, inline link and platform link resolves', () => {
    for (const l of LESSONS) {
      for (const r of l.related) {
        assert.ok(learn.getLessonByKey(r), `${key(l)} → related ${r}`);
        assert.notEqual(r, key(l), `${key(l)} relates to itself`);
      }
      for (const s of strings(l)) for (const m of s.matchAll(/\[([^\]]+)\]\(([^)\s]+)\)/g)) assert.ok(resolves(m[2]), `${key(l)}: link ${m[2]}`);
      for (const p of l.platformLinks ?? []) assert.ok(resolves(p.href), `${key(l)}: platform link ${p.href}`);
    }
  });

  it('markup is balanced and strings carry no stray escapes', () => {
    for (const l of LESSONS) {
      for (const s of strings(l)) {
        assert.doesNotMatch(s, /[\u0000-\u0008\u000b-\u001f]/, `${key(l)}: control character (a "\\t" that should be "\\\\t"?) in: ${s.slice(0, 60)}`);
        assert.equal((s.match(/\\\(/g) ?? []).length, (s.match(/\\\)/g) ?? []).length, `${key(l)}: unbalanced \\( \\) in ${s.slice(0, 60)}`);
        assert.equal((s.match(/\*\*/g) ?? []).length % 2, 0, `${key(l)}: unbalanced ** in ${s.slice(0, 60)}`);
        assert.equal((s.match(/`/g) ?? []).length % 2, 0, `${key(l)}: unbalanced \` in ${s.slice(0, 60)}`);
      }
      for (const b of l.blocks) if (b.type === 'formula') assert.doesNotMatch(b.tex, /[\u0000-\u001f]/, `${key(l)}: formula escapes`);
    }
  });

  it('formulas render with KaTeX without errors', () => {
    const katex = require(path.join(root, 'node_modules/katex'));
    const inline = [];
    for (const l of LESSONS) {
      for (const b of l.blocks) if (b.type === 'formula') katex.renderToString(b.tex, { displayMode: true, throwOnError: true, strict: 'ignore' });
      for (const s of strings(l)) for (const m of s.matchAll(/\\\((.+?)\\\)/g)) inline.push([key(l), m[1]]);
    }
    for (const [k, tex] of inline) assert.doesNotThrow(() => katex.renderToString(tex, { throwOnError: true, strict: 'ignore' }), `${k}: ${tex}`);
    assert.ok(inline.length > 30);
  });

  it('diagrams and interactive examples used by lessons exist', () => {
    const diagrams = src('components/learn/Diagrams.tsx');
    const interactive = src('components/learn/Interactive.tsx');
    for (const l of LESSONS) {
      for (const b of l.blocks) {
        if (b.type === 'diagram') assert.match(diagrams, new RegExp(`'?${b.id}'?: [A-Z]`), `${key(l)}: diagram ${b.id}`);
        if (b.type === 'interactive') assert.match(interactive, new RegExp(`'${b.id}': [A-Z]`), `${key(l)}: widget ${b.id}`);
      }
    }
  });

  it('previous / next navigation walks every lesson in order', () => {
    assert.equal(learn.neighbours(LESSONS[0]).prev, undefined);
    assert.equal(learn.neighbours(LESSONS.at(-1)).next, undefined);
    for (let i = 1; i < LESSONS.length - 1; i++) {
      const { prev, next } = learn.neighbours(LESSONS[i]);
      assert.equal(key(prev), key(LESSONS[i - 1]));
      assert.equal(key(next), key(LESSONS[i + 1]));
    }
  });

  it('strategy lessons only describe strategies the platform supports', () => {
    const builtins = fs.readdirSync(path.join(root, '../../crates/spectra-backtest/src/strategies')).filter((f) => f !== 'mod.rs').map((f) => f.replace('.rs', ''));
    assert.deepEqual(builtins.sort(), ['buy_and_hold', 'equal_weight', 'sma_cross', 'zscore_reversion']);
    const dslFns = src('../../crates/spectra-engine/src/dsl/validator.rs');
    for (const f of ['sma', 'ema', 'rsi', 'target_shares', 'stop_sell', 'limit_sell', 'cancel_order']) assert.match(dslFns, new RegExp(`"${f}"`), `rule language has ${f}`);
  });
});

describe('Metric registry', () => {
  const entries = Object.entries(metricDefinitions);

  it('every Learn More link opens a real lesson', () => {
    for (const [id, d] of entries) {
      assert.ok(resolves(d.learnPath) && d.learnPath.split('/').length === 4, `${id}: ${d.learnPath}`);
      assert.ok(d.name && d.shortDescription.length >= 30 && d.shortDescription.length <= 220, `${id}: description length`);
    }
  });

  it('every metric lesson is reachable from the registry', () => {
    const linked = new Set(entries.map(([, d]) => d.learnPath));
    for (const l of learn.lessonsIn('metrics')) assert.ok(linked.has(`/learn/metrics/${l.slug}`), `metrics/${l.slug} has no registry entry`);
  });

  it('every metric id used in the Studio is in the registry, and results views use it widely', () => {
    const files = walk(path.join(root, 'components')).filter((f) => f.endsWith('.tsx'));
    const used = new Map();
    for (const f of files) {
      const text = fs.readFileSync(f, 'utf8');
      for (const m of text.matchAll(/metric(?:=|: )["']([a-z0-9_]+)["']|(?:undefined|\)), '([a-z0-9_]+)'\]/g)) {
        const id = m[1] ?? m[2];
        if (!id) continue;
        used.set(path.relative(root, f).replace(/\\/g, '/'), [...(used.get(path.relative(root, f).replace(/\\/g, '/')) ?? []), id]);
      }
    }
    const all = [...used.values()].flat();
    for (const id of all) if (id in metricDefinitions === false && /_/.test(id)) assert.fail(`unknown metric id ${id}`);
    const count = (f) => new Set(used.get(f) ?? []).size;
    assert.ok(count('components/studio/backtests/RunDetail.tsx') >= 35, 'backtest results: every metric has an ⓘ');
    assert.ok(count('components/studio/RiskLab.tsx') >= 8, 'risk lab');
    assert.ok(count('components/studio/PaperLab.tsx') >= 5, 'paper trading');
    for (const f of ['components/studio/OptimizationLab.tsx', 'components/studio/StressPanel.tsx', 'components/studio/BacktestToPaperGap.tsx', 'components/studio/StudioShell.tsx']) {
      assert.match(src(f), /InfoTip|MetricLabel/, `${f} uses the registry`);
    }
    const has = (f, ids) => {
      for (const id of ids) assert.ok((used.get(f) ?? []).includes(id), `${f}: no ⓘ for ${id}`);
    };
    has('components/studio/BenchmarkLab.tsx', ['latency_p50', 'parallel_speedup', 'paths_per_second']);
    has('components/studio/OptimizationLab.tsx', ['psr', 'dsr', 'expected_max_sharpe', 'sharpe_variance', 'num_trials']);
    has('components/studio/BacktestToPaperGap.tsx', ['slippage', 'timing_latency', 'slippage_drift', 'pnl_gap']);
    has('components/studio/RiskCenter.tsx', ['gross_exposure', 'net_exposure']);
    // Every registry metric is shown somewhere with its ⓘ.
    const studio = files.map((f) => fs.readFileSync(f, 'utf8')).join('\n');
    for (const id of Object.keys(metricDefinitions)) {
      assert.ok(new RegExp(`["']${id}["']`).test(studio), `${id} is in the registry but no component shows it`);
    }
    has('components/studio/OverviewDashboard.tsx', ['gross_exposure_utilization', 'net_exposure_utilization', 'leverage_utilization', 'drawdown_utilization', 'daily_loss_utilization']);
  });

  it('tooltips render from the registry, keyboard- and screen-reader-accessible', () => {
    const tip = src('components/ui/MetricInfo.tsx');
    assert.match(tip, /metricDefinitions\[metric\]/, 'text comes from the registry');
    assert.match(tip, /aria-describedby=\{descId\}/, 'description available to screen readers');
    assert.match(tip, /aria-expanded=\{open\}/);
    assert.match(tip, /e\.key === 'Escape'/);
    assert.match(tip, /:focus-visible/, 'opens on keyboard focus');
    assert.match(tip, /pointerType !== 'mouse'/, 'hover only for mice; taps toggle');
    assert.match(tip, /href=\{def\.learnPath\}/, 'Learn More uses the registry path');
  });
});

describe('Calculations match the engine', () => {
  const near = (a, b, eps = 1e-12) => assert.ok(Math.abs(a - b) < eps, `${a} vs ${b}`);

  it('equity metrics: the engine\'s own unit-test values (spectra_engine::metrics tests)', () => {
    const m = calc.equityMetrics([100, 110, 99, 121], 3, 0);
    assert.equal(m.periods, 3);
    near(m.totalReturn, 0.21);
    near(m.annualizedReturn, 0.21);
    near(m.maxDrawdown, 0.1);
    const r = [0.1, 99 / 110 - 1, 121 / 99 - 1];
    const mean = r.reduce((a, b) => a + b) / 3;
    const sd = Math.sqrt(r.reduce((a, x) => a + (x - mean) ** 2, 0) / 2);
    near(m.sharpe, (mean / sd) * Math.sqrt(3));
    near(m.annualizedVolatility, sd * Math.sqrt(3));
    const flat = calc.equityMetrics([100, 100, 100, 100], 252, 0);
    near(flat.sharpe, 0);
    near(flat.annualizedVolatility, 0);
    near(calc.equityMetrics([100, 110], 126, 0).annualizedReturn, 1.1 ** 126 - 1, 1e-6);
    assert.equal(calc.equityMetrics([100], 252, 0).valid, false);
    const wiped = calc.equityMetrics([100, 0], 252, 0);
    near(wiped.totalReturn, -1);
    near(wiped.maxDrawdown, 1);
  });

  it('reproduces a real engine run: metrics, drawdown, tail and Sortino from its equity curve', () => {
    const fx = JSON.parse(src('scripts/fixtures/learn-engine-run.json'));
    const m = calc.equityMetrics(fx.equity, fx.periods_per_year, fx.risk_free_rate);
    const e = fx.expected;
    const close = (a, b, what) => assert.ok(Math.abs(a - b) <= 1e-9 * Math.max(1, Math.abs(b)), `${what}: ${a} vs engine ${b}`);
    assert.equal(m.periods, e.metrics.periods);
    close(m.totalReturn, e.metrics.total_return, 'total return');
    close(m.annualizedReturn, e.metrics.annualized_return, 'annualized return');
    close(m.annualizedVolatility, e.metrics.annualized_volatility, 'volatility');
    close(m.sharpe, e.metrics.sharpe, 'sharpe');
    close(m.maxDrawdown, e.metrics.max_drawdown, 'max drawdown');
    close(m.sortino, e.sortino, 'sortino');
    const d = calc.drawdownStats(fx.equity);
    close(d.currentDrawdown, e.drawdown.current_drawdown, 'current drawdown');
    assert.equal(d.longestUnderwater, e.drawdown.longest_underwater_periods);
    close(d.ulcerIndex, e.drawdown.ulcer_index, 'ulcer index');
    const t = calc.tailStats(calc.returnsFromEquity(fx.equity), e.tail.level);
    close(t.var, e.tail.var, 'VaR');
    close(t.es, e.tail.expected_shortfall, 'expected shortfall');
    close(t.worst, e.tail.worst_return, 'worst period');
  });

  it('historical VaR follows the engine\'s nearest-rank rule', () => {
    const r = Array.from({ length: 100 }, (_, i) => i / 100 - 0.5);
    const t = calc.tailStats(r, 0.95);
    near(t.var, 0.46);
    near(t.es, 0.48);
    assert.equal(calc.tailStats(r.slice(0, 19)).valid, false);
    assert.equal(calc.nearestRankIndex(100, 0.05), 4);
  });

  it('drawdown statistics: the engine\'s test case', () => {
    const d = calc.drawdownStats([100, 120, 90, 95, 130, 117]);
    near(d.maxDrawdown, 0.25);
    assert.equal(d.longestUnderwater, 2);
    near(d.currentDrawdown, 0.1);
  });

  it('trade statistics follow compute_trade_stats', () => {
    const t = calc.tradeStats([400, -100, 350, -150, -120, 250, -80, -90, 500, -60]);
    assert.equal(t.wins, 4);
    near(t.winRate, 0.4);
    near(t.profitFactor, 2.5);
    near(t.payoffRatio, 3.75);
    near(t.expectancy, 90);
    assert.equal(calc.tradeStats([10, 20]).profitFactor, Infinity, 'no losses: infinite (shown as —)');
    assert.equal(calc.tradeStats([10, 0, -5]).wins + calc.tradeStats([10, 0, -5]).losses, 2, 'break-even trades are neither');
  });

  it('costs follow effective_slippage_bps and commission_fee', () => {
    near(calc.effectiveBps(2, 4, 10, 100, 20000), 4.05);
    near(calc.commission(0, 1, 1, 100, 5000), 1);
    near(calc.commission(0.005, 1, 0, 100, 5000), 1);
  });

  it('worked examples in the lessons state the numbers the formulas give', () => {
    const text = (k) => learn.lessonText(learn.getLessonByKey(k));
    const sharpe = ((0.0005 - 0.04 / 252) / 0.01) * Math.sqrt(252);
    assert.equal(sharpe.toFixed(2), '0.54');
    assert.match(text('metrics/sharpe-ratio'), /0\.54/);
    assert.equal((Math.sqrt(1.125) - 1).toFixed(4), '0.0607');
    assert.match(text('metrics/annualized-return'), /6\.07%/);
    assert.equal((1.331 ** (1 / 3) - 1).toFixed(4), '0.1000');
    assert.equal(((0.0005 / 0.006) * Math.sqrt(252)).toFixed(2), '1.32');
    assert.match(text('metrics/sortino-ratio'), /1\.32/);
    assert.equal((0.01 * Math.sqrt(252)).toFixed(3), '0.159');
    near((130000 - 97500) / 130000, 0.25);
    assert.match(text('metrics/maximum-drawdown'), /25%/);
    const t = calc.tradeStats([400, -100, 350, -150, -120, 250, -80, -90, 500, -60]);
    assert.match(text('metrics/win-rate'), /40%/);
    assert.match(text('metrics/profit-factor'), new RegExp(`${t.profitFactor}`));
    assert.match(text('metrics/risk-reward-ratio'), /3\.75/);
    assert.match(text('metrics/expectancy'), /\$90/);
    const dd = calc.drawdownStats([100, 120, 90, 95, 130, 117]);
    assert.match(text('metrics/drawdown-duration'), new RegExp(`${Math.round(dd.currentDrawdown * 100)}%`));
    // One-day 95% VaR of $1M at 20% volatility under the risk engine's GBM model.
    const z = -1.6448536269514722;
    const sigma = 0.2;
    const T = 1 / 252;
    const varGbm = 1e6 * (1 - Math.exp(-(sigma ** 2 / 2) * T + sigma * Math.sqrt(T) * z));
    assert.equal(Math.round(varGbm / 100) * 100, 20600);
    assert.match(text('metrics/value-at-risk'), /\$20,600/);
    // MinTRL example.
    const mintrl = 1 + 1.005 * (1.6448536269514722 / 0.1) ** 2;
    assert.equal(Math.round(mintrl), 273);
    assert.match(text('metrics/minimum-track-record-length'), /273/);
    // Slippage example fill.
    assert.equal((50 * (1 + calc.effectiveBps(2, 4, 10, 100, 20000) / 1e4)).toFixed(2), '50.02');
    assert.match(text('metrics/slippage'), /\$50\.02/);
  });

  it('parses pasted numbers, including thousands separators', () => {
    assert.deepEqual(calc.parseNumbers('100, 105 98.5\n110'), [100, 105, 98.5, 110]);
    assert.deepEqual(calc.parseNumbers('$100,000 $1,002,500 -40'), [100000, 1002500, -40]);
  });
});

describe('Search', () => {
  const index = learn.searchIndex();
  const top = (q, n = 1) => searchLessons(index, q, n).map((e) => e.key);

  it('finds the obvious lesson first', () => {
    if (process.env.DEBUG_SEARCH) for (const q of ['sharpe', 'maximum drawdown', 'sortino', 'candlestick']) console.log(q, top(q, 4));
    assert.deepEqual(top('sharpe'), ['metrics/sharpe-ratio']);
    assert.deepEqual(top('maximum drawdown'), ['metrics/maximum-drawdown']);
    assert.deepEqual(top('sortino'), ['metrics/sortino-ratio']);
    assert.deepEqual(top('candlestick'), ['paper-trading/reading-candlestick-charts']);
    assert.ok(top('limit order', 3).includes('fundamentals/market-and-limit-orders'));
    assert.ok(top('bid ask spread', 3).includes('fundamentals/bid-ask-spread'));
  });

  it('understands common abbreviations and requires every term', () => {
    assert.ok(top('pnl', 5).some((k) => k.includes('pnl') || k.includes('profit-and-loss')));
    assert.ok(top('mdd', 3).includes('metrics/maximum-drawdown'));
    assert.ok(top('cvar', 3).includes('metrics/expected-shortfall'));
    assert.deepEqual(searchLessons(index, '', 5), []);
    assert.deepEqual(searchLessons(index, 'zzqqxx', 5), []);
    for (const e of searchLessons(index, 'alpaca keys', 10)) assert.match(`${e.title} ${e.summary} ${e.keywords} ${e.body}`.toLowerCase(), /alpaca/);
  });
});

describe('Guided tour', () => {
  it('auto-starts once per user and tour version, never after finishing or skipping', () => {
    const { shouldAutoStart, EMPTY_ONBOARDING, applyOnboarding, TOUR_VERSION } = onboarding;
    assert.equal(shouldAutoStart(null), true, 'unknown user state: new user');
    assert.equal(shouldAutoStart(EMPTY_ONBOARDING), true);
    const midway = applyOnboarding(EMPTY_ONBOARDING, 'progress', 'paper');
    assert.equal(shouldAutoStart(midway), true, 'left half way: offered again');
    const done = applyOnboarding(midway, 'complete', null);
    assert.equal(shouldAutoStart(done), false);
    const skipped = applyOnboarding(EMPTY_ONBOARDING, 'skip', 'welcome');
    assert.equal(shouldAutoStart(skipped), false, 'Skip for Now is remembered');
    assert.equal(shouldAutoStart(done, TOUR_VERSION + 1), true, 'a new tour version is shown once');
    assert.equal(applyOnboarding(done, 'skip', 'dashboard').onboardingCompleted, true, 'leaving a replay early keeps it finished');
    assert.equal(applyOnboarding({ ...done, onboardingVersion: 5 }, 'complete', null, 2).onboardingVersion, 5, 'version never goes down');
  });

  it('normalizes untrusted state and keeps users apart', () => {
    const { normalizeOnboarding, onboardingStorageKey } = onboarding;
    assert.deepEqual(normalizeOnboarding('junk'), onboarding.EMPTY_ONBOARDING);
    assert.deepEqual(normalizeOnboarding({ onboardingCompleted: 'yes', onboardingVersion: -3, lastCompletedStep: '<script>' }), onboarding.EMPTY_ONBOARDING);
    assert.notEqual(onboardingStorageKey('a@x.com'), onboardingStorageKey('b@x.com'));
    assert.equal(onboardingStorageKey(' A@X.com '), onboardingStorageKey('a@x.com'));
  });

  it('introduces the features that exist, in the requested order, with targets that are in the UI', () => {
    assert.deepEqual(MAIN_TOUR.map((s) => s.id), ['dashboard', 'simulations', 'strategies', 'results', 'risk', 'risk-center', 'portfolio', 'data', 'optimization', 'benchmark', 'registry', 'settings', 'engine', 'paper', 'learn', 'account', 'admin']);
    const ids = [...MAIN_TOUR, ...Object.values(PAGE_TOURS).flat()].map((s) => s.id);
    assert.equal(new Set(ids).size, ids.length, 'unique step ids');
    const ui = walk(path.join(root, 'components')).filter((f) => f.endsWith('.tsx')).map((f) => fs.readFileSync(f, 'utf8')).join('\n');
    const targets = new Set([...ui.matchAll(/data-tour="([a-z-]+)"/g)].map((m) => m[1]));
    if (ui.includes('data-tour={`nav-${t.id}`}')) for (const t of STUDIO_TABS) targets.add(`nav-${t}`);
    for (const s of [...MAIN_TOUR, ...Object.values(PAGE_TOURS).flat()]) {
      assert.match(s.id, onboarding.STEP_ID, `${s.id}: storable step id`);
      assert.ok(s.targets.some((t) => targets.has(t)), `${s.id}: no target rendered (${s.targets})`);
      for (const t of s.targets) assert.ok(targets.has(t), `${s.id}: target ${t} is not in the UI`);
      if (s.tab) assert.ok(STUDIO_TABS.includes(s.tab), `${s.id}: tab ${s.tab}`);
      if (s.requiresTab) assert.ok(STUDIO_TABS.includes(s.requiresTab), `${s.id}: requires ${s.requiresTab}`);
    }
    for (const t of Object.keys(PAGE_TOURS)) assert.ok(STUDIO_TABS.includes(t), `page tour for ${t}`);
  });

  it('never trades, submits or changes app state: it only highlights and saves its own progress', () => {
    const code = (f) => src(f).replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
    const tour = code('components/onboarding/OnboardingTour.tsx') + code('components/onboarding/useOnboardingState.ts') + code('components/onboarding/HelpMenu.tsx');
    assert.doesNotMatch(tour, /alpaca|createAlpacaOrder|backtests'|\/orders|submit|\.click\(\)|requestSubmit|logout|openAuthModal/i, 'no trading, submitting or auth calls');
    const fetches = [...tour.matchAll(/fetch\('([^']+)'/g)].map((m) => m[1]);
    assert.ok(fetches.length >= 2);
    for (const f of fetches) assert.equal(f, '/api/auth/onboarding', 'the only request is its own state');
    assert.match(tour, /disableActiveInteraction: true/, 'highlighted elements cannot be clicked during a step');
    assert.match(tour, /allowClose: true/, 'dismissible at any time');
    assert.match(tour, /Skip tour/);
    assert.match(tour, /showProgress: true/);
    assert.match(tour, /Start Tour/);
    assert.match(tour, /Skip for Now/);
    assert.match(tour, /Go to Dashboard/);
    assert.match(tour, /Explore Learning Center/);
  });

  it('can be restarted from the Help menu and the Learning Center', () => {
    assert.match(src('components/onboarding/HelpMenu.tsx'), /Take the tour/);
    assert.match(src('components/studio/StudioShell.tsx'), /<HelpMenu tab=\{tab\} \/>/);
    assert.match(src('components/onboarding/TourLauncher.tsx'), /\/studio\?tour=1/);
    assert.match(src('app/learn/page.tsx'), /<TourLauncher \/>/);
    assert.match(src('components/onboarding/OnboardingTour.tsx'), /params\.get\('tour'\) === '1'/);
  });
});
