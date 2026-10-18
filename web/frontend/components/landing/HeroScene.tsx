'use client';

import { useEffect, useRef } from 'react';
import { useReducedMotion } from 'framer-motion';
import { useTheme } from '@/lib/theme';

/** Runs `draw(t)` on a DPR-aware canvas, pausing while offscreen; one static frame for reduced motion. */
function useCanvasLoop(ref: React.RefObject<HTMLCanvasElement>, draw: (ctx: CanvasRenderingContext2D, w: number, h: number, t: number) => void, deps: unknown[]) {
  const reduce = useReducedMotion();
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    let raf = 0;
    let visible = true;
    let w = 0;
    let h = 0;
    const resize = () => {
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const r = canvas.getBoundingClientRect();
      w = r.width;
      h = r.height;
      canvas.width = Math.max(1, Math.floor(w * dpr));
      canvas.height = Math.max(1, Math.floor(h * dpr));
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    resize();
    const ro = new ResizeObserver(() => {
      resize();
      if (reduce) draw(ctx, w, h, 0);
    });
    ro.observe(canvas);
    const io = new IntersectionObserver(([e]) => {
      visible = e.isIntersecting;
      if (visible && !reduce) loop(performance.now());
    });
    io.observe(canvas);
    const loop = (now: number) => {
      cancelAnimationFrame(raf);
      if (!visible) return;
      draw(ctx, w, h, now / 1000);
      raf = requestAnimationFrame(loop);
    };
    if (reduce) draw(ctx, w, h, 0);
    else raf = requestAnimationFrame(loop);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      io.disconnect();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reduce, ...deps]);
}

function rng(seed: number) {
  return () => {
    seed = (seed * 1664525 + 1013904223) >>> 0;
    return seed / 4294967296;
  };
}

/** A tilted two-arm spiral galaxy with a warm core and differential rotation. */
export function GalaxyCanvas({ className }: { className?: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const { theme } = useTheme();
  const dark = theme === 'dark';
  const stars = useRef<{ r: number; a: number; c: string; s: number; o: number }[]>([]);
  if (!stars.current.length) {
    const rand = rng(7);
    const warm = ['#FFE7B8', '#FFD08A', '#FFC06B', '#FFF4DE'];
    const cool = ['#9FC2FF', '#B9A4FF', '#7FE7FF', '#D7CCFF', '#FFFFFF'];
    for (let i = 0; i < 6500; i++) {
      const arm = i % 2;
      const r = Math.pow(rand(), 0.75);
      const spread = (rand() + rand() - 1) * (0.5 - r * 0.25);
      const a = arm * Math.PI + r * 5.2 + spread;
      const core = r < 0.18 || (r < 0.32 && rand() < 0.5);
      stars.current.push({ r: r + (rand() - 0.5) * 0.04, a, c: core ? warm[Math.floor(rand() * warm.length)] : cool[Math.floor(rand() * cool.length)], s: core ? 0.7 + rand() * 1.3 : 0.5 + rand() * 1.1, o: 0.35 + rand() * 0.65 });
    }
  }

  useCanvasLoop(
    ref,
    (ctx, w, h, t) => {
      ctx.clearRect(0, 0, w, h);
      const cx = w * 0.56;
      const cy = h * 0.42;
      const R = Math.min(w, h * 1.9) * 0.48;
      const tilt = 0.42;
      const rot = -0.35;
      ctx.globalCompositeOperation = dark ? 'lighter' : 'source-over';
      // Core glow
      const g = ctx.createRadialGradient(cx, cy, 0, cx, cy, R * 0.32);
      g.addColorStop(0, dark ? 'rgba(255,240,210,1)' : 'rgba(251,191,36,0.6)');
      g.addColorStop(0.25, dark ? 'rgba(255,190,110,0.45)' : 'rgba(245,158,11,0.22)');
      g.addColorStop(1, 'rgba(255,170,90,0)');
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.ellipse(cx, cy, R * 0.38, R * 0.38 * tilt * 1.4, rot, 0, Math.PI * 2);
      ctx.fill();
      // Diffuse disk
      const d = ctx.createRadialGradient(cx, cy, R * 0.1, cx, cy, R);
      d.addColorStop(0, dark ? 'rgba(130,110,255,0.22)' : 'rgba(99,102,241,0.10)');
      d.addColorStop(1, 'rgba(80,60,200,0)');
      ctx.fillStyle = d;
      ctx.beginPath();
      ctx.ellipse(cx, cy, R, R * tilt, rot, 0, Math.PI * 2);
      ctx.fill();
      const cr = Math.cos(rot);
      const sr = Math.sin(rot);
      for (const s of stars.current) {
        const ang = s.a + t * (0.05 / Math.sqrt(0.08 + s.r));
        const x0 = Math.cos(ang) * s.r * R;
        const y0 = Math.sin(ang) * s.r * R * tilt;
        const x = cx + x0 * cr - y0 * sr;
        const y = cy + x0 * sr + y0 * cr;
        ctx.globalAlpha = dark ? s.o : s.o * 0.55;
        ctx.fillStyle = dark ? s.c : s.r < 0.22 ? '#D97706' : '#4F46E5';
        ctx.fillRect(x, y, s.s, s.s);
      }
      ctx.globalAlpha = 1;
      ctx.globalCompositeOperation = 'source-over';
    },
    [dark],
  );
  return <canvas ref={ref} className={className} aria-hidden />;
}

/** Undulating perspective dot-surface (cyan → violet) with floating candlesticks. */
export function WavesCanvas({ className }: { className?: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const { theme } = useTheme();
  const dark = theme === 'dark';
  const candles = useRef<{ x: number; body: number; wick: number; up: boolean; phase: number }[]>([]);
  if (!candles.current.length) {
    const rand = rng(11);
    for (let i = 0; i < 34; i++) candles.current.push({ x: rand() * 2 - 1, body: 10 + rand() * 34, wick: 6 + rand() * 18, up: rand() > 0.38, phase: rand() * Math.PI * 2 });
  }

  useCanvasLoop(
    ref,
    (ctx, w, h, t) => {
      ctx.clearRect(0, 0, w, h);
      const COLS = Math.min(110, Math.max(40, Math.floor(w / 14)));
      const ROWS = 34;
      const height = (x: number, z: number) => 0.12 * Math.sin(2.6 * x + t * 0.6) + 0.06 * Math.sin(4.5 * z - t * 0.8 + x) + 0.035 * Math.sin(6.5 * x + 3 * z + t * 0.45);
      const project = (x: number, z: number, y: number) => {
        const s = 1 / (1 + z * 1.7);
        return { sx: w / 2 + x * w * 0.62 * s, sy: h * 0.35 + z * h * 0.55 * s - y * h * 1.25 * s, s };
      };
      ctx.globalCompositeOperation = dark ? 'lighter' : 'source-over';
      const c1 = dark ? [34, 211, 238] : [8, 145, 178];
      const c2 = dark ? [139, 92, 246] : [99, 102, 241];
      for (let r = ROWS - 1; r >= 0; r--) {
        const z = 1 - r / (ROWS - 1);
        ctx.beginPath();
        let first = true;
        for (let c = 0; c < COLS; c++) {
          const x = (c / (COLS - 1)) * 2.4 - 1.2;
          const p = project(x, z, height(x, z));
          const k = (x + 1.2) / 2.4;
          const col = c1.map((v, i) => Math.round(v + (c2[i] - v) * k));
          const a = (dark ? 1 : 0.7) * (1 - z * 0.6);
          ctx.fillStyle = `rgba(${col[0]},${col[1]},${col[2]},${a})`;
          const rad = 2.1 * p.s;
          ctx.fillRect(p.sx - rad / 2, p.sy - rad / 2, rad, rad);
          if (first) ctx.moveTo(p.sx, p.sy);
          else ctx.lineTo(p.sx, p.sy);
          first = false;
        }
        if (r % 2 === 0) {
          ctx.strokeStyle = dark ? `rgba(56,189,248,${0.22 * (1 - z * 0.7)})` : `rgba(2,132,199,${0.16 * (1 - z * 0.7)})`;
          ctx.lineWidth = 1;
          ctx.stroke();
        }
      }
      // Candlesticks riding the back of the surface.
      for (const cd of candles.current) {
        const z = 0.78;
        const p = project(cd.x, z, height(cd.x, z));
        const bob = Math.sin(t * 0.8 + cd.phase) * 4;
        const top = p.sy - 26 - cd.body + bob;
        const col = cd.up ? (dark ? 'rgba(34,211,238,0.8)' : 'rgba(8,145,178,0.6)') : dark ? 'rgba(167,139,250,0.75)' : 'rgba(99,102,241,0.55)';
        ctx.strokeStyle = col;
        ctx.fillStyle = col;
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(p.sx, top - cd.wick);
        ctx.lineTo(p.sx, top + cd.body + cd.wick * 0.6);
        ctx.stroke();
        ctx.globalAlpha = 0.85;
        ctx.fillRect(p.sx - 3, top, 6, cd.body);
        ctx.globalAlpha = 1;
      }
      ctx.globalCompositeOperation = 'source-over';
    },
    [dark],
  );
  return <canvas ref={ref} className={className} aria-hidden />;
}
