import type { Metadata, Viewport } from 'next';
import { ThemeProvider, themeInitScript } from '@/lib/theme';
import './globals.css';

export const metadata: Metadata = {
  title: { default: 'Spectra Labs: Institutional quant research & real-time portfolio risk', template: '%s · Spectra Labs' },
  description:
    'A pure-Rust Monte Carlo risk engine, an event-driven backtester without look-ahead bias, overfitting validation (DSR, PBO) and a SHA-256 hash-chained experiment registry.',
};

export const viewport: Viewport = {
  themeColor: [
    { media: '(prefers-color-scheme: dark)', color: '#050811' },
    { media: '(prefers-color-scheme: light)', color: '#F8FAFC' },
  ],
};

const scrollResetScript = `(function(){try{if('scrollRestoration' in history){history.scrollRestoration='manual'}}catch(e){}})();`;

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    // `dark` is the default; the init script restores a stored "light" choice before paint.
    <html lang="en" className="dark font-sans" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeInitScript }} />
        <script dangerouslySetInnerHTML={{ __html: scrollResetScript }} />
      </head>
      <body className="min-h-screen">
        <ThemeProvider>{children}</ThemeProvider>
      </body>
    </html>
  );
}
