/** @type {import('next').NextConfig} */
// The project lives on a Windows-mounted path for WSL users. Give WSL its own
// build cache so it cannot collide with a Windows Next.js process using .next.
const isWsl = process.platform === 'linux' && Boolean(process.env.WSL_DISTRO_NAME);
const defaultDistDir = isWsl ? '.next-wsl' : '.next';

const nextConfig = {
  distDir: process.env.NEXT_DIST_DIR || defaultDistDir,
};

module.exports = nextConfig;
