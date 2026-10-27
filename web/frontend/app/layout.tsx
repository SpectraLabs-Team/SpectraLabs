import type { Metadata, Viewport } from 'next';
import { headers } from 'next/headers';
import { AuthModal } from '@/components/auth/AuthModal';
import { AuthProvider } from '@/lib/auth';
import { ActivityTracker } from '@/components/activity/ActivityTracker';
import { ThemeProvider, themeInitScript } from '@/lib/theme';
import { ToastProvider } from '@/components/ui/StudioToast';
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

const scrollResetScript = `(function(){try{if('scrollRestoration' in history){history.scrollRestoration='manual'}if(window.location.hash){history.replaceState(null,'',window.location.pathname+window.location.search)}window.scrollTo(0,0)}catch(e){}})();`;

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  // Per-request nonce from middleware (Content-Security-Policy): our two inline scripts carry it,
  // so they still run when the policy only allows nonced scripts.
  const headerList = await headers();
  const nonce = headerList.get('x-nonce') ?? undefined;
  return (
    // `dark` is the default; the init script restores a stored "light" choice before paint.
    <html lang="en" className="dark font-sans" suppressHydrationWarning>
      <head>
        <script nonce={nonce} dangerouslySetInnerHTML={{ __html: themeInitScript }} />
        <script nonce={nonce} dangerouslySetInnerHTML={{ __html: scrollResetScript }} />
      </head>
      <body className="min-h-screen">
        <ThemeProvider>
          <AuthProvider>
            <ToastProvider>
              {children}
              <AuthModal />
              <ActivityTracker />
            </ToastProvider>
          </AuthProvider>
        </ThemeProvider>
      </body>
    </html>
  );
}
