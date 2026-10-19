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

// Security headers for every response (lib/securityHeaders.js). The Content-Security-Policy is
// set per request in middleware.ts, because it carries a nonce.
const { pageHeaders, apiHeaders } = require('./lib/securityHeaders');

const nextConfig = {
  distDir: process.env.NEXT_DIST_DIR || defaultDistDir,
  poweredByHeader: false,
  // Lint runs separately (`npm run lint`, once eslint is installed); don't fail or warn in builds.
  eslint: { ignoreDuringBuilds: true },
  async headers() {
    return [
      { source: '/:path*', headers: pageHeaders() },
      { source: '/api/:path*', headers: apiHeaders },
    ];
  },
  async redirects() {
    return legacy.map(([source, tab]) => ({ source, destination: studio(tab), permanent: false }));
  },
  serverExternalPackages: ['@node-rs/argon2', 'pg'],
};

// Refuse to start (dev or production server) without a real AUTH_SECRET and a DATABASE_URL. Builds are not checked,
// so CI can compile without secrets. Rules match authSecret() in lib/server/authStore.ts.
const { PHASE_DEVELOPMENT_SERVER, PHASE_PRODUCTION_SERVER } = require('next/constants');
const RETIRED_AUTH_SECRETS = new Set(['spectralabs_jwt_default_secret_key_32bytes_required']);

function assertServerEnv() {
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
  if (!process.env.DATABASE_URL || !process.env.DATABASE_URL.trim()) {
    console.error('\n  ✖ DATABASE_URL is not set.\n    Run `npm run setup`, then see "Database" in web/README.md.\n');
    process.exit(1);
  }
}

module.exports = (phase) => {
  if (phase === PHASE_DEVELOPMENT_SERVER || phase === PHASE_PRODUCTION_SERVER) assertServerEnv();
  return nextConfig;
};
