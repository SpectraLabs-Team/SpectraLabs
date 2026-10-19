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
  // The Docker image runs the standalone server (docker/frontend.Dockerfile sets NEXT_OUTPUT).
  ...(process.env.NEXT_OUTPUT === 'standalone' ? { output: 'standalone' } : {}),
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

module.exports = nextConfig;
