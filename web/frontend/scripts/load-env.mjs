// Load web/frontend/.env* into process.env for scripts, the same way `next dev` does.
// Values already set in the shell win.
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

export const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export function loadEnv() {
  try {
    const { loadEnvConfig } = createRequire(import.meta.url)('@next/env');
    loadEnvConfig(root, process.env.NODE_ENV !== 'production', { info() {}, error: console.error });
    return;
  } catch {
    // @next/env ships with next; fall back to a plain .env.local read if it isn't installed yet.
  }
  const file = path.join(root, '.env.local');
  if (!fs.existsSync(file)) return;
  for (const line of fs.readFileSync(file, 'utf8').split(/\r?\n/)) {
    const m = line.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)\s*$/);
    if (m && process.env[m[1]] === undefined) process.env[m[1]] = m[2].replace(/^(['"])(.*)\1$/, '$2');
  }
}
