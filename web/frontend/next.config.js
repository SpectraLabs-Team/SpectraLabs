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

const nextConfig = {
  distDir: process.env.NEXT_DIST_DIR || defaultDistDir,
  async redirects() {
    return legacy.map(([source, tab]) => ({ source, destination: studio(tab), permanent: false }));
  },
};

module.exports = nextConfig;
