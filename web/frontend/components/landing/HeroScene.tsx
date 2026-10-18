'use client';

import { useEffect, useRef } from 'react';
import { useReducedMotion } from 'framer-motion';

/**
 * A very light twinkling-dust overlay for the hero backdrop: ~70 points that fade in and out.
 * Pauses while offscreen; draws nothing for reduced motion (the artwork already has stars).
 */
export function DustOverlay({ className }: { className?: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const reduce = useReducedMotion();

  useEffect(() => {
    const canvas = ref.current;
    const ctx = canvas?.getContext('2d');
    if (!canvas || !ctx || reduce) return;
    let seed = 1234567;
    const rand = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 4294967296);
    const motes = Array.from({ length: 70 }, () => ({ x: rand(), y: rand() * 0.75, r: 0.4 + rand() * 1.1, phase: rand() * Math.PI * 2, speed: 0.3 + rand() * 0.9, drift: (rand() - 0.5) * 0.004, hue: rand() > 0.5 ? '186,230,255' : '214,196,255' }));
    let w = 0;
    let h = 0;
    let raf = 0;
    let visible = true;
    const resize = () => {
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const r = canvas.getBoundingClientRect();
      w = r.width;
      h = r.height;
      canvas.width = Math.floor(w * dpr);
      canvas.height = Math.floor(h * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    const frame = (now: number) => {
      const t = now / 1000;
      ctx.clearRect(0, 0, w, h);
      for (const m of motes) {
        const a = 0.15 + 0.55 * (0.5 + 0.5 * Math.sin(t * m.speed + m.phase));
        const x = ((m.x + m.drift * t) % 1 + 1) % 1;
        ctx.fillStyle = `rgba(${m.hue},${a})`;
        ctx.beginPath();
        ctx.arc(x * w, m.y * h, m.r, 0, Math.PI * 2);
        ctx.fill();
      }
      if (visible) raf = requestAnimationFrame(frame);
    };
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    const io = new IntersectionObserver(([e]) => {
      visible = e.isIntersecting;
      cancelAnimationFrame(raf);
      if (visible) raf = requestAnimationFrame(frame);
    });
    io.observe(canvas);
    raf = requestAnimationFrame(frame);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      io.disconnect();
    };
  }, [reduce]);

  return <canvas ref={ref} className={className} aria-hidden />;
}
