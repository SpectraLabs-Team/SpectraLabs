import type { DiagramId } from '@/lib/learn/types';

/**
 * Lesson diagrams as inline SVG. Colours come from the theme tokens (fill-ink, stroke-accent, …)
 * so every diagram works in light and dark mode without separate artwork.
 */

const T = 'fill-ink/70 text-[11px]';
const TS = 'fill-ink/50 text-[10px]';

function Frame({ label, children, h = 220 }: { label: string; children: React.ReactNode; h?: number }) {
  return (
    <svg viewBox={`0 0 640 ${h}`} role="img" aria-label={label} className="h-auto w-full font-sans">
      {children}
    </svg>
  );
}

function OrderBook() {
  const asks = [
    [100.04, 900],
    [100.03, 500],
    [100.02, 650],
    [100.01, 300],
  ];
  const bids = [
    [99.99, 400],
    [99.98, 700],
    [99.97, 550],
    [99.96, 1000],
  ];
  const w = (q: number) => (q / 1000) * 220;
  return (
    <Frame label="Order book with asks above and bids below the spread" h={260}>
      <text x={20} y={22} className={T}>Asks (sellers)</text>
      {asks.map(([p, q], i) => (
        <g key={p}>
          <rect x={320} y={30 + i * 24} width={w(q)} height={18} rx={3} className="fill-loss/25" />
          <text x={308} y={43 + i * 24} textAnchor="end" className="num fill-loss text-[11px]">{p.toFixed(2)}</text>
          <text x={326 + w(q)} y={43 + i * 24} className={TS}>{q}</text>
        </g>
      ))}
      <line x1={120} x2={600} y1={134} y2={134} className="stroke-ink/20" strokeDasharray="4 4" />
      <text x={20} y={138} className="fill-accent text-[11px] font-semibold">Spread 0.02 (2 bps)</text>
      <text x={20} y={152} className={TS}>mid 100.00</text>
      {bids.map(([p, q], i) => (
        <g key={p}>
          <rect x={320} y={146 + i * 24} width={w(q)} height={18} rx={3} className="fill-profit/25" />
          <text x={308} y={159 + i * 24} textAnchor="end" className="num fill-profit text-[11px]">{p.toFixed(2)}</text>
          <text x={326 + w(q)} y={159 + i * 24} className={TS}>{q}</text>
        </g>
      ))}
      <text x={20} y={250} className={T}>Bids (buyers)</text>
      <text x={600} y={22} textAnchor="end" className={TS}>bar length = shares resting at that price</text>
    </Frame>
  );
}

function Box({ x, y, w = 120, h = 40, label, sub, tone = 'ink' }: { x: number; y: number; w?: number; h?: number; label: string; sub?: string; tone?: 'ink' | 'accent' | 'profit' | 'loss' | 'caution' }) {
  const cls = {
    ink: 'fill-ink/[0.04] stroke-ink/25',
    accent: 'fill-accent/10 stroke-accent/60',
    profit: 'fill-profit/10 stroke-profit/60',
    loss: 'fill-loss/10 stroke-loss/60',
    caution: 'fill-caution/10 stroke-caution/60',
  }[tone];
  return (
    <g>
      <rect x={x} y={y} width={w} height={h} rx={8} className={cls} strokeWidth={1.2} />
      <text x={x + w / 2} y={y + (sub ? h / 2 - 2 : h / 2 + 4)} textAnchor="middle" className="fill-ink text-[11.5px] font-medium">{label}</text>
      {sub && <text x={x + w / 2} y={y + h / 2 + 12} textAnchor="middle" className={TS}>{sub}</text>}
    </g>
  );
}

function Arrow({ x1, y1, x2, y2, label, m }: { x1: number; y1: number; x2: number; y2: number; label?: string; m: string }) {
  return (
    <g>
      <line x1={x1} y1={y1} x2={x2} y2={y2} className="stroke-ink/40" strokeWidth={1.3} markerEnd={`url(#${m})`} />
      {label && <text x={(x1 + x2) / 2} y={(y1 + y2) / 2 - 5} textAnchor="middle" className={TS}>{label}</text>}
    </g>
  );
}

function Defs({ m }: { m: string }) {
  return (
    <defs>
      <marker id={m} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
        <path d="M0,0 L10,5 L0,10 z" className="fill-ink/50" />
      </marker>
    </defs>
  );
}

function OrderLifecycle() {
  return (
    <Frame label="Order states: pending, partially filled, filled, cancelled, rejected, expired" h={230}>
      <Defs m="arrow-lifecycle" />
      <Box x={20} y={90} label="Submitted" sub="by the strategy" />
      <Box x={190} y={90} label="Pending" sub="waiting for a bar" tone="accent" />
      <Box x={360} y={30} label="Partially filled" sub="rest still open" tone="caution" />
      <Box x={520} y={90} w={100} label="Filled" tone="profit" />
      <Box x={360} y={150} w={80} h={34} label="Cancelled" />
      <Box x={450} y={150} w={80} h={34} label="Rejected" tone="loss" />
      <Box x={540} y={150} w={80} h={34} label="Expired" />
      <Arrow m="arrow-lifecycle" x1={140} y1={110} x2={188} y2={110} />
      <Arrow m="arrow-lifecycle" x1={310} y1={100} x2={358} y2={60} />
      <Arrow m="arrow-lifecycle" x1={310} y1={110} x2={518} y2={110} label="fills completely" />
      <Arrow m="arrow-lifecycle" x1={480} y1={70} x2={540} y2={90} />
      <Arrow m="arrow-lifecycle" x1={300} y1={130} x2={390} y2={150} />
      <Arrow m="arrow-lifecycle" x1={290} y1={130} x2={480} y2={150} />
      <Arrow m="arrow-lifecycle" x1={280} y1={130} x2={570} y2={150} />
      <text x={20} y={210} className={TS}>Filled, cancelled, rejected and expired are terminal states.</text>
    </Frame>
  );
}

function EnginePipeline() {
  return (
    <Frame label="Engine pipeline from market data to metrics" h={200}>
      <Defs m="arrow-pipeline" />
      <Box x={10} y={20} w={110} label="Market data" sub="versioned bars" />
      <Box x={140} y={20} w={110} label="Strategy" sub="on_bar / on_fill" tone="accent" />
      <Box x={270} y={20} w={110} label="Orders" sub="price-indexed book" />
      <Box x={400} y={20} w={110} label="Execution" sub="fill rules + costs" tone="accent" />
      <Box x={530} y={20} w={100} label="Ledger" sub="cash, positions" />
      <Arrow m="arrow-pipeline" x1={120} y1={40} x2={138} y2={40} />
      <Arrow m="arrow-pipeline" x1={250} y1={40} x2={268} y2={40} />
      <Arrow m="arrow-pipeline" x1={380} y1={40} x2={398} y2={40} />
      <Arrow m="arrow-pipeline" x1={510} y1={40} x2={528} y2={40} />
      <Box x={400} y={110} w={110} label="Risk checks" sub="leverage, limits" tone="caution" />
      <Box x={530} y={110} w={100} label="Metrics" sub="summary, charts" tone="profit" />
      <Box x={140} y={110} w={240} label="Experiment registry" sub="fingerprints, hash chain" />
      <Arrow m="arrow-pipeline" x1={455} y1={110} x2={455} y2={62} label="" />
      <Arrow m="arrow-pipeline" x1={580} y1={62} x2={580} y2={108} />
      <Arrow m="arrow-pipeline" x1={528} y1={130} x2={382} y2={130} />
      <text x={10} y={185} className={TS}>Orders placed on bar T fill on a later bar; the strategy never sees prices it could not have known.</text>
    </Frame>
  );
}

function SimulationLoop() {
  const steps = ['1. Fill orders from earlier bars', '2. Mark positions at the close', '3. Record an equity point', '4. Strategy sees the bar, places orders'];
  const cx = 320;
  const cy = 120;
  const r = 78;
  const pos = [
    [cx, cy - r],
    [cx + r * 1.9, cy],
    [cx, cy + r],
    [cx - r * 1.9, cy],
  ];
  return (
    <Frame label="The backtest event loop" h={240}>
      <ellipse cx={cx} cy={cy} rx={r * 1.9} ry={r} className="fill-none stroke-ink/15" strokeDasharray="5 5" />
      <text x={cx} y={cy - 4} textAnchor="middle" className="fill-accent text-[12px] font-semibold">each timestamp T</text>
      <text x={cx} y={cy + 12} textAnchor="middle" className={TS}>then T+1, T+2, …</text>
      {steps.map((s, i) => (
        <g key={s}>
          <rect x={pos[i][0] - 105} y={pos[i][1] - 15} width={210} height={30} rx={15} className={i === 0 ? 'fill-accent/10 stroke-accent/60' : 'fill-panel stroke-ink/25'} />
          <text x={pos[i][0]} y={pos[i][1] + 4} textAnchor="middle" className="fill-ink text-[11px]">{s}</text>
        </g>
      ))}
    </Frame>
  );
}

function path(values: number[], x0: number, w: number, y0: number, h: number, min: number, max: number) {
  return values.map((v, i) => `${i === 0 ? 'M' : 'L'}${(x0 + (i / (values.length - 1)) * w).toFixed(1)},${(y0 + h - ((v - min) / (max - min)) * h).toFixed(1)}`).join(' ');
}

function Drawdown() {
  const eq = [100, 104, 110, 108, 118, 130, 124, 112, 101, 97.5, 104, 115, 126, 133, 140];
  const peak: number[] = [];
  eq.forEach((v, i) => peak.push(Math.max(v, i ? peak[i - 1] : v)));
  const [x0, w, y0, h, min, max] = [40, 560, 20, 160, 90, 145];
  const X = (i: number) => x0 + (i / (eq.length - 1)) * w;
  const Y = (v: number) => y0 + h - ((v - min) / (max - min)) * h;
  const area = `M${X(5)},${Y(130)} ${[6, 7, 8, 9, 10, 11, 12].map((i) => `L${X(i)},${Y(eq[i])}`).join(' ')} L${X(13)},${Y(133)} L${X(13)},${Y(130)} Z`;
  return (
    <Frame label="Equity curve with running peak and the maximum drawdown shaded" h={220}>
      <path d={area} className="fill-loss/15" />
      <path d={path(peak, x0, w, y0, h, min, max)} className="fill-none stroke-ink/35" strokeDasharray="4 4" strokeWidth={1.4} />
      <path d={path(eq, x0, w, y0, h, min, max)} className="fill-none stroke-accent" strokeWidth={2.2} />
      <line x1={X(9)} x2={X(9)} y1={Y(130)} y2={Y(97.5)} className="stroke-loss" strokeWidth={1.5} />
      <circle cx={X(5)} cy={Y(130)} r={3.5} className="fill-ink/60" />
      <circle cx={X(9)} cy={Y(97.5)} r={3.5} className="fill-loss" />
      <text x={X(5)} y={Y(130) - 8} textAnchor="middle" className={T}>peak 130k</text>
      <text x={X(9) + 6} y={Y(97.5) + 14} className="fill-loss text-[11px]">trough 97.5k</text>
      <text x={X(9) + 8} y={Y(114)} className="fill-loss text-[11px] font-semibold">−25%</text>
      <text x={x0} y={212} className={TS}>Solid: equity · dashed: running peak · shaded: the maximum drawdown</text>
    </Frame>
  );
}

function SmaCrossover() {
  const price = [50, 49, 48.5, 49.5, 51, 53, 55, 56, 58, 57, 59, 61, 60, 58, 55, 53, 51, 50, 49, 50.5, 52];
  const sma = (n: number) => price.map((_, i) => { const s = price.slice(Math.max(0, i - n + 1), i + 1); return s.reduce((a, b) => a + b, 0) / s.length; });
  const fast = sma(3);
  const slow = sma(8);
  const [x0, w, y0, h, min, max] = [40, 560, 20, 150, 46, 63];
  const X = (i: number) => x0 + (i / (price.length - 1)) * w;
  let start = -1;
  const regions: [number, number][] = [];
  fast.forEach((f, i) => {
    const up = f > slow[i] && i > 0;
    if (up && start < 0) start = i;
    if ((!up || i === fast.length - 1) && start >= 0) {
      regions.push([start, i]);
      start = -1;
    }
  });
  return (
    <Frame label="Price with fast and slow moving averages; shaded where the strategy is long" h={210}>
      {regions.map(([a, b]) => (
        <rect key={a} x={X(a)} y={y0} width={X(b) - X(a)} height={h} className="fill-profit/10" />
      ))}
      <path d={path(price, x0, w, y0, h, min, max)} className="fill-none stroke-ink/40" strokeWidth={1.4} />
      <path d={path(slow, x0, w, y0, h, min, max)} className="fill-none stroke-violet" strokeWidth={2} />
      <path d={path(fast, x0, w, y0, h, min, max)} className="fill-none stroke-accent" strokeWidth={2} />
      <text x={x0} y={198} className={TS}>
        <tspan className="fill-ink/60">grey: price</tspan> · <tspan className="fill-accent">fast SMA</tspan> · <tspan className="fill-violet">slow SMA</tspan> · green: long
      </text>
    </Frame>
  );
}

function MeanReversion() {
  const price = [100, 101, 99, 98, 96, 95.5, 97, 99, 100.5, 102, 103.5, 104, 102, 100, 99, 100];
  const [x0, w, y0, h, min, max] = [40, 560, 20, 150, 93, 107];
  const Y = (v: number) => y0 + h - ((v - min) / (max - min)) * h;
  return (
    <Frame label="Price oscillating around its mean with bands at plus and minus two standard deviations" h={210}>
      <rect x={x0} y={Y(104)} width={w} height={Y(96) - Y(104)} className="fill-accent/[0.06]" />
      {[104, 100, 96].map((v) => (
        <line key={v} x1={x0} x2={x0 + w} y1={Y(v)} y2={Y(v)} className={v === 100 ? 'stroke-ink/40' : 'stroke-accent/50'} strokeDasharray={v === 100 ? undefined : '5 4'} />
      ))}
      <text x={x0 + w + 4} y={Y(104) + 4} className={TS}>+2σ</text>
      <text x={x0 + w + 4} y={Y(100) + 4} className={TS}>mean</text>
      <text x={x0 + w + 4} y={Y(96) + 4} className={TS}>−2σ</text>
      <path d={path(price, x0, w, y0, h, min, max)} className="fill-none stroke-ink/70" strokeWidth={2} />
      <circle cx={x0 + (5 / 15) * w} cy={Y(95.5)} r={5} className="fill-profit" />
      <text x={x0 + (5 / 15) * w + 8} y={Y(95.5) + 4} className="fill-profit text-[11px]">enter long (z &lt; −2)</text>
      <circle cx={x0 + (8 / 15) * w} cy={Y(100.5)} r={5} className="fill-ink/60" />
      <text x={x0 + (8 / 15) * w + 8} y={Y(100.5) + 16} className={T}>exit (z back above −0.5)</text>
    </Frame>
  );
}

function Candlestick() {
  const c = (x: number, o: number, hi: number, lo: number, cl: number, label: string) => {
    const up = cl >= o;
    const Y = (v: number) => 20 + (60 - v) * 8;
    return (
      <g>
        <line x1={x} x2={x} y1={Y(hi)} y2={Y(lo)} className={up ? 'stroke-profit' : 'stroke-loss'} strokeWidth={2} />
        <rect x={x - 22} y={Y(Math.max(o, cl))} width={44} height={Math.abs(Y(o) - Y(cl))} rx={2} className={up ? 'fill-profit/80' : 'fill-loss/80'} />
        <text x={x + 34} y={Y(hi) + 4} className={TS}>high {hi}</text>
        <text x={x + 34} y={Y(Math.max(o, cl)) + 4} className={TS}>{up ? `close ${cl}` : `open ${o}`}</text>
        <text x={x + 34} y={Y(Math.min(o, cl)) + 4} className={TS}>{up ? `open ${o}` : `close ${cl}`}</text>
        <text x={x + 34} y={Y(lo) + 4} className={TS}>low {lo}</text>
        <text x={x} y={210} textAnchor="middle" className={T}>{label}</text>
      </g>
    );
  };
  return (
    <Frame label="A rising candle and a falling candle with open, high, low and close labelled" h={220}>
      {c(170, 44, 58, 40, 54, 'Rising: close above open')}
      {c(450, 55, 57, 38, 43, 'Falling: close below open')}
    </Frame>
  );
}

function Spread() {
  const X = (p: number) => 60 + (p - 99.95) * 5200;
  return (
    <Frame label="Bid, mid and ask prices on a line, with the spread between bid and ask" h={130}>
      <line x1={40} x2={600} y1={60} y2={60} className="stroke-ink/25" />
      <rect x={X(99.98)} y={48} width={X(100.02) - X(99.98)} height={24} className="fill-accent/10" />
      {[
        [99.98, 'Bid 99.98', 'you sell here', 'fill-profit'],
        [100.0, 'Mid 100.00', '', 'fill-ink/70'],
        [100.02, 'Ask 100.02', 'you buy here', 'fill-loss'],
      ].map(([p, a, b, cls]) => (
        <g key={String(p)}>
          <circle cx={X(p as number)} cy={60} r={5} className={cls as string} />
          <text x={X(p as number)} y={38} textAnchor="middle" className="num fill-ink text-[11.5px]">{a as string}</text>
          {b && <text x={X(p as number)} y={92} textAnchor="middle" className={TS}>{b as string}</text>}
        </g>
      ))}
      <text x={320} y={118} textAnchor="middle" className="fill-accent text-[11px]">spread 0.04 = 4 bps of mid · each market fill pays half (2 bps)</text>
    </Frame>
  );
}

const DIAGRAMS: Record<DiagramId, () => React.JSX.Element> = {
  'order-book': OrderBook,
  'order-lifecycle': OrderLifecycle,
  'engine-pipeline': EnginePipeline,
  'simulation-loop': SimulationLoop,
  drawdown: Drawdown,
  'sma-crossover': SmaCrossover,
  'mean-reversion': MeanReversion,
  candlestick: Candlestick,
  spread: Spread,
};

export function Diagram({ id, caption }: { id: DiagramId; caption?: string }) {
  const D = DIAGRAMS[id];
  return (
    <figure className="my-6 overflow-hidden rounded-xl border border-ink/[0.08] bg-ink/[0.015] p-3 sm:p-4">
      <div className="overflow-x-auto">
        <div className="min-w-[520px]">
          <D />
        </div>
      </div>
      {caption && <figcaption className="mt-2 text-center text-[11.5px] text-ink/50">{caption}</figcaption>}
    </figure>
  );
}
