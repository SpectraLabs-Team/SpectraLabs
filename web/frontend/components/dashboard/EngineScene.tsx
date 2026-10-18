"use client";

import { Activity, ArrowRight, BarChart3, Database } from 'lucide-react';
import { motion, useReducedMotion } from 'framer-motion';

const steps = [
  { icon: Database, title: 'Portfolio', tag: 'INPUT' },
  { icon: Activity, title: 'Simulation', tag: 'ENGINE' },
  { icon: BarChart3, title: 'Risk report', tag: 'OUTPUT' },
];

export default function EngineScene() {
  const reduce = useReducedMotion();
  return <motion.div
    initial={reduce ? false : { opacity: 0, y: 20, rotateX: 5 }}
    animate={{ opacity: 1, y: 0, rotateX: 0 }}
    transition={{ duration: 0.75, ease: 'easeOut' }}
    whileHover={reduce ? undefined : { rotateY: -3, rotateX: 2, scale: 1.015 }}
    style={{ transformStyle: 'preserve-3d', perspective: 1000 }}
    className="engine-scene relative min-h-[280px] overflow-hidden rounded-2xl border border-white/10 bg-[#0b1625] p-5 shadow-2xl shadow-blue-950/30 sm:min-h-[320px] sm:p-7"
    aria-label="Illustration of the Spectra Labs portfolio, simulation, and risk report workflow"
  >
    <div className="scene-grid absolute inset-0 opacity-50" aria-hidden="true" />
    <div className="scene-glow absolute -right-14 -top-20 h-64 w-64 rounded-full bg-blue-500/20 blur-3xl" aria-hidden="true" />
    <div className="relative flex items-start justify-between gap-3">
      <div><p className="text-[10px] font-semibold uppercase tracking-[0.24em] text-blue-300/80">Spectra engine</p><p className="mt-1 text-sm text-white/60">Risk workflow</p></div>
      <span className="rounded-full border border-white/10 bg-white/5 px-2.5 py-1 text-[10px] uppercase tracking-wider text-white/50">Architecture view</span>
    </div>

    <div className="relative mt-9 grid grid-cols-[1fr_auto_1fr_auto_1fr] items-center gap-2 sm:mt-12 sm:gap-3" style={{ transform: 'translateZ(26px)' }}>
      {steps.map(({ icon: Icon, title, tag }, index) => <div key={title} className="contents">
        <motion.div
          initial={reduce ? false : { opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.16 + index * 0.14, duration: 0.45 }}
          className={`scene-node scene-node-${index + 1} relative z-10 flex min-w-0 flex-col items-center rounded-xl border border-white/10 bg-slate-900/80 px-2 py-3 text-center shadow-xl backdrop-blur sm:px-3 sm:py-4`}
        >
          <span className="mb-2 rounded-lg bg-blue-400/10 p-2 text-blue-200"><Icon size={17} /></span>
          <span className="text-[9px] font-medium uppercase tracking-[0.16em] text-blue-200/60">{tag}</span>
          <span className="mt-1 truncate text-[11px] font-medium text-white sm:text-xs">{title}</span>
        </motion.div>
        {index < steps.length - 1 && <div className="relative z-0 flex items-center text-blue-300/60"><motion.div
          initial={{ scaleX: 0 }} animate={{ scaleX: 1 }} transition={{ delay: 0.35 + index * 0.15, duration: 0.55 }}
          className="h-px w-4 origin-left bg-gradient-to-r from-blue-300/70 to-cyan-200 sm:w-8" />{!reduce && <motion.span animate={{ x: [0, 18], opacity: [0, 1, 0] }} transition={{ duration: 1.6, repeat: Infinity, delay: index * 0.4 }} className="absolute left-0"><ArrowRight size={12} /></motion.span>}</div>}
      </div>)}
    </div>

    <div className="relative mt-6 flex items-center gap-3 sm:mt-8" style={{ transform: 'translateZ(12px)' }}>
      <div className="flex h-7 w-7 items-center justify-center rounded-full border border-cyan-300/20 bg-cyan-300/10"><span className="h-1.5 w-1.5 rounded-full bg-cyan-300 shadow-[0_0_12px_3px_rgba(103,232,249,0.45)]" /></div>
      <div><p className="text-[10px] font-medium text-white/70">Connected workflow</p><p className="mt-0.5 text-[9px] text-white/35">Portfolio data → Monte Carlo → metric layer</p></div>
      {!reduce && <motion.div className="ml-auto h-px w-14 bg-gradient-to-r from-transparent via-cyan-300/70 to-transparent" animate={{ opacity: [0.25, 1, 0.25], scaleX: [0.7, 1.15, 0.7] }} transition={{ duration: 2.2, repeat: Infinity }} />}
    </div>
    {!reduce && <motion.div aria-hidden="true" className="scene-orbit absolute right-12 top-20 h-16 w-16 rounded-full border border-cyan-200/15" animate={{ rotate: 360 }} transition={{ duration: 22, repeat: Infinity, ease: 'linear' }}><span className="absolute -left-1 top-1/2 h-2 w-2 -translate-y-1/2 rounded-full bg-cyan-200 shadow-[0_0_14px_3px_rgba(103,232,249,0.6)]" /></motion.div>}
  </motion.div>;
}
