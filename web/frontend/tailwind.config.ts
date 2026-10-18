import type { Config } from 'tailwindcss';

// Semantic colours are CSS variables (RGB triplets) defined in app/globals.css for
// light (:root) and dark (.dark). Components use `bg-canvas`, `text-ink/60`, … and
// pick up the active theme; `dark:` variants are used where a theme needs its own hue.
const v = (name: string) => `rgb(var(--${name}) / <alpha-value>)`;

const config: Config = {
  darkMode: 'class',
  content: ['./app/**/*.{ts,tsx}', './components/**/*.{ts,tsx}', './lib/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        canvas: v('canvas'),
        panel: v('panel'),
        ink: v('ink'),
        accent: { DEFAULT: v('accent'), ice: v('accent-ice'), electric: '#00F2FE', violet: v('violet') },
        profit: v('profit'),
        loss: v('loss'),
        caution: v('caution'),
        onaccent: '#03131A',
      },
      fontFamily: {
        sans: ['Inter', '-apple-system', 'BlinkMacSystemFont', 'Segoe UI', 'Roboto', 'system-ui', 'sans-serif'],
        mono: ['"JetBrains Mono"', 'SF Mono', 'ui-monospace', 'Menlo', 'monospace'],
      },
      boxShadow: {
        glow: '0 0 0 1px rgb(var(--accent) / 0.35), 0 0 24px -4px rgb(var(--accent) / 0.45)',
        'glow-lg': '0 0 0 1px rgb(var(--accent) / 0.4), 0 10px 50px -10px rgb(var(--accent) / 0.55)',
        panel: 'var(--panel-shadow)',
      },
      backgroundImage: {
        'accent-gradient': 'linear-gradient(135deg, #00F2FE 0%, #38BDF8 55%, #4F8BFF 100%)',
      },
      keyframes: {
        'pulse-dot': { '0%,100%': { opacity: '1', transform: 'scale(1)' }, '50%': { opacity: '0.45', transform: 'scale(0.85)' } },
        shimmer: { from: { backgroundPosition: '200% 0' }, to: { backgroundPosition: '-200% 0' } },
        float: { '0%,100%': { transform: 'translateY(0)' }, '50%': { transform: 'translateY(-8px)' } },
      },
      animation: {
        'pulse-dot': 'pulse-dot 2s ease-in-out infinite',
        shimmer: 'shimmer 2.4s linear infinite',
        float: 'float 7s ease-in-out infinite',
      },
    },
  },
  plugins: [],
};

export default config;
