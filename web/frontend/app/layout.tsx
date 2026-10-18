import type { Metadata, Viewport } from 'next';
import { Inter, JetBrains_Mono } from 'next/font/google';
import './globals.css';

const sans = Inter({ subsets: ['latin'], variable: '--font-sans', display: 'swap' });
const mono = JetBrains_Mono({ subsets: ['latin'], variable: '--font-mono', display: 'swap' });

export const metadata: Metadata = {
  title: { default: 'Spectra Labs: Evidence-first quant research & portfolio risk', template: '%s · Spectra Labs' },
  description:
    'A pure-Rust Monte Carlo risk engine, an event-driven backtester without look-ahead bias, overfitting validation (DSR, PBO) and a tamper-evident experiment registry.',
};

export const viewport: Viewport = { themeColor: '#07090E' };

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className={`${sans.variable} ${mono.variable}`}>
      <body className="min-h-screen bg-obsidian-950">{children}</body>
    </html>
  );
}
