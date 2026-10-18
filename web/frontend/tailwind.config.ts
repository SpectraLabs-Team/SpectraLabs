import type { Config } from 'tailwindcss';

const config: Config = {
  content: ['./app/**/*.{ts,tsx}', './components/**/*.{ts,tsx}', './lib/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        obsidian: { 950: '#07090E', 900: '#0B0F17', 850: '#0E131D', 800: '#121826' },
        cyan: { electric: '#00F2FE' },
        accent: { DEFAULT: '#22D3EE', ice: '#38BDF8', electric: '#00F2FE' },
        profit: '#10B981',
        loss: '#F43F5E',
        caution: '#F59E0B',
      },
      fontFamily: {
        sans: ['var(--font-sans, Inter)', 'ui-sans-serif', 'system-ui', 'sans-serif'],
        mono: ['var(--font-mono, "JetBrains Mono")', 'SF Mono', 'ui-monospace', 'monospace'],
      },
      boxShadow: {
        glow: '0 0 0 1px rgba(34,211,238,0.35), 0 0 24px -4px rgba(34,211,238,0.45)',
        'glow-lg': '0 0 0 1px rgba(34,211,238,0.4), 0 10px 50px -10px rgba(0,242,254,0.55)',
        panel: '0 1px 0 0 rgba(255,255,255,0.04) inset, 0 24px 48px -24px rgba(0,0,0,0.8)',
      },
      backgroundImage: {
        'accent-gradient': 'linear-gradient(135deg, #00F2FE 0%, #38BDF8 55%, #4F8BFF 100%)',
      },
      keyframes: {
        'pulse-dot': { '0%,100%': { opacity: '1', transform: 'scale(1)' }, '50%': { opacity: '0.45', transform: 'scale(0.85)' } },
        shimmer: { from: { backgroundPosition: '200% 0' }, to: { backgroundPosition: '-200% 0' } },
        'border-spin': { to: { '--angle': '360deg' } },
      },
      animation: {
        'pulse-dot': 'pulse-dot 2s ease-in-out infinite',
        shimmer: 'shimmer 2.4s linear infinite',
      },
    },
  },
  plugins: [],
};

export default config;
