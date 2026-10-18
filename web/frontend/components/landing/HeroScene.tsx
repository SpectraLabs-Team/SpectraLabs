'use client';

import { useEffect, useRef } from 'react';
import { useReducedMotion } from 'framer-motion';

/**
 * Twinkling cosmic stardust and sparkle overlay.
 * Enriches the sky, especially the upper-right cosmic quadrant with twinkling stars and cross sparkles.
 */
export function DustOverlay({ className }: { className?: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const reduce = useReducedMotion();

  useEffect(() => {
    const canvas = ref.current;
    const ctx = canvas?.getContext('2d');
    if (!canvas || !ctx || reduce) return;

    let seed = 424242;
    const rand = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 4294967296);

    // 1. General ambient space dust across the sky
    const generalMotes = Array.from({ length: 60 }, () => ({
      x: rand(),
      y: rand() * 0.7,
      r: 0.5 + rand() * 1.1,
      phase: rand() * Math.PI * 2,
      speed: 0.4 + rand() * 1.1,
      drift: (rand() - 0.5) * 0.003,
      hue: rand() > 0.4 ? '186,230,255' : '224,204,255',
      sparkle: false,
    }));

    // 2. Dense upper-right cosmic stars (filling the area right of the galaxy)
    const upperRightStars = Array.from({ length: 50 }, () => ({
      x: 0.72 + rand() * 0.27,
      y: 0.02 + rand() * 0.38,
      r: 0.6 + rand() * 1.5,
      phase: rand() * Math.PI * 2,
      speed: 0.5 + rand() * 1.4,
      drift: (rand() - 0.5) * 0.002,
      hue: rand() > 0.6 ? '255,240,210' : rand() > 0.3 ? '186,230,255' : '230,210,255',
      sparkle: rand() < 0.22,
    }));

    const allStars = [...generalMotes, ...upperRightStars];

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

      for (const s of allStars) {
        const pulse = 0.5 + 0.5 * Math.sin(t * s.speed + s.phase);
        const a = 0.2 + 0.7 * pulse;
        const x = (((s.x + s.drift * t) % 1) + 1) % 1;
        const px = x * w;
        const py = s.y * h;

        ctx.fillStyle = `rgba(${s.hue}, ${a})`;
        ctx.beginPath();
        ctx.arc(px, py, s.r, 0, Math.PI * 2);
        ctx.fill();

        // 4-point cross-diffraction sparkle on select stars
        if (s.sparkle && pulse > 0.6) {
          const spA = (pulse - 0.6) / 0.4;
          ctx.strokeStyle = `rgba(${s.hue}, ${spA * 0.75})`;
          ctx.lineWidth = 0.8;
          ctx.beginPath();
          const spLen = 4 + s.r * 2.5;
          ctx.moveTo(px - spLen, py);
          ctx.lineTo(px + spLen, py);
          ctx.moveTo(px, py - spLen);
          ctx.lineTo(px, py + spLen);
          ctx.stroke();
        }
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
