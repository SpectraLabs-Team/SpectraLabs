export const GITHUB_URL = 'https://github.com/SpectraLabs-Team/spectralabs';

export const NAV: { href: string; label: string; external?: boolean }[] = [
  { href: '/#features', label: 'Features' },
  { href: '/#taxonomy', label: 'Taxonomy' },
  { href: '/#benchmarks', label: 'Benchmarks' },
  { href: '/architecture', label: 'Architecture' },
];

/** Developer references: footer only, kept out of the product header. */
export const DEV_LINKS: { href: string; label: string; external?: boolean }[] = [
  { href: '/architecture', label: 'Docs' },
  { href: GITHUB_URL, label: 'GitHub', external: true },
];
