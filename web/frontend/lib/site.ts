export const GITHUB_URL = 'https://github.com/SpectraLabs-Team/spectralabs';

export const NAV: { href: string; label: string; external?: boolean }[] = [
  { href: '/studio', label: 'Product' },
  { href: '/#features', label: 'Features' },
  { href: '/#benchmarks', label: 'Benchmarks' },
  { href: '/#architecture', label: 'Architecture' },
  { href: `${GITHUB_URL}#readme`, label: 'Docs', external: true },
  { href: GITHUB_URL, label: 'GitHub', external: true },
];
