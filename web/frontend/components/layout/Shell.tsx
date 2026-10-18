"use client";

import Sidebar from './Sidebar';
import StatusBar from './StatusBar';
import { usePathname } from 'next/navigation';
import { motion, useReducedMotion } from 'framer-motion';

export default function Shell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const reduce = useReducedMotion();
  return (
    <div className="flex h-screen flex-col overflow-hidden bg-background bg-grid md:flex-row">
      <Sidebar />
      <div className="flex-1 flex flex-col overflow-hidden">
        <StatusBar />
        <main className="min-h-0 flex-1 overflow-auto">
          <motion.div key={pathname} initial={reduce ? false : { opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.24, ease: 'easeOut' }}>
            {children}
          </motion.div>
        </main>
      </div>
    </div>
  );
}
