/** @type {import('next').NextConfig} */
// The project lives on a Windows-mounted path for WSL users. Give WSL its own
// build cache so it cannot collide with a Windows Next.js process using .next.
const isWsl = process.platform === 'linux' && Boolean(process.env.WSL_DISTRO_NAME);
const defaultDistDir = isWsl ? '.next-wsl' : '.next';

// The old multi-page dashboard was folded into /studio. Keep its URLs working.
const studio = (tab) => (tab ? `/studio?tab=${tab}` : '/studio');
const legacy = [
  ['/risk-lab', 'risk'],
  ['/stress', 'risk'],
  ['/risk/:path*', 'risk'],
  ['/simulation/:path*', 'risk'],
  ['/portfolio', 'portfolio'],
  ['/benchmark', 'benchmark'],
  ['/compute/:path*', 'benchmark'],
  ['/market', null],
  ['/system/:path*', null],
];

// Baseline security headers for every response. (A full Content-Security-Policy is a later
// step: the theme/scroll init scripts in app/layout.tsx are inline and would need nonces.)
const securityHeaders = [
  { key: 'X-Frame-Options', value: 'DENY' }, // no clickjacking via iframes
  { key: 'X-Content-Type-Options', value: 'nosniff' },
  { key: 'Referrer-Policy', value: 'strict-origin-when-cross-origin' },
  { key: 'Permissions-Policy', value: 'camera=(), microphone=(), geolocation=(), payment=()' },
  ...(process.env.NODE_ENV === 'production'
    ? [{ key: 'Strict-Transport-Security', value: 'max-age=31536000; includeSubDomains' }]
    : []),
];

const nextConfig = {
  distDir: process.env.NEXT_DIST_DIR || defaultDistDir,
  poweredByHeader: false,
  // Lint runs separately (`npm run lint`, once eslint is installed); don't fail or warn in builds.
  eslint: { ignoreDuringBuilds: true },
  async headers() {
    return [
      { source: '/:path*', headers: securityHeaders },
      // Auth and engine responses are per-user; never let a shared cache keep them.
      { source: '/api/:path*', headers: [{ key: 'Cache-Control', value: 'no-store' }] },
    ];
  },
  async redirects() {
    return legacy.map(([source, tab]) => ({ source, destination: studio(tab), permanent: false }));
  },
};

// Refuse to start (dev or production server) without a real AUTH_SECRET. Builds are not checked,
// so CI can compile without secrets. Rules match authSecret() in lib/server/authStore.ts.
const { PHASE_DEVELOPMENT_SERVER, PHASE_PRODUCTION_SERVER } = require('next/constants');
const RETIRED_AUTH_SECRETS = new Set(['spectralabs_jwt_default_secret_key_32bytes_required']);

function assertAuthSecret() {
  require('@next/env').loadEnvConfig(__dirname); // .env.local etc.; a no-op if already loaded
  const s = process.env.AUTH_SECRET;
  let problem = null;
  if (!s || !s.trim()) problem = 'AUTH_SECRET is not set';
  else if (s.length < 32) problem = 'AUTH_SECRET is too short (need 32+ characters)';
  else if (RETIRED_AUTH_SECRETS.has(s)) problem = 'AUTH_SECRET is the old public development default';
  if (problem) {
    console.error(`\n  ✖ ${problem}.\n    Run \`npm run setup\` in web/frontend to generate one, then start again.\n`);
    process.exit(1);
  }
}

module.exports = (phase) => {
  if (phase === PHASE_DEVELOPMENT_SERVER || phase === PHASE_PRODUCTION_SERVER) assertAuthSecret();
  return nextConfig;
};
