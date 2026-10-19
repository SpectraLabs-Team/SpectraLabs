// Create or repair web/frontend/.env.local. Run: npm run setup
// - Copies .env.example to .env.local if it doesn't exist.
// - Fills AUTH_SECRET with 48 random bytes when it is empty, too short or the old public default.
// - Never overwrites a value that is already good, never prints secrets.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const examplePath = path.join(root, '.env.example');
const envPath = path.join(root, '.env.local');
const RETIRED = new Set(['spectralabs_jwt_default_secret_key_32bytes_required']);

const created = !fs.existsSync(envPath);
let text = created ? fs.readFileSync(examplePath, 'utf8') : fs.readFileSync(envPath, 'utf8');

function get(key) {
  const m = text.match(new RegExp(`^${key}=(.*)$`, 'm'));
  return m ? m[1].trim().replace(/^["']|["']$/g, '') : undefined;
}
function set(key, value) {
  const line = `${key}=${value}`;
  text = new RegExp(`^${key}=.*$`, 'm').test(text)
    ? text.replace(new RegExp(`^${key}=.*$`, 'm'), line)
    : `${text.replace(/\n*$/, '\n')}${line}\n`;
}

const changes = [];
const current = get('AUTH_SECRET');
if (!current || current.length < 32 || RETIRED.has(current)) {
  set('AUTH_SECRET', crypto.randomBytes(48).toString('base64url'));
  changes.push(current ? 'replaced weak AUTH_SECRET (existing sessions are signed out)' : 'generated AUTH_SECRET');
}

fs.writeFileSync(envPath, text, { mode: 0o600 });
try { fs.chmodSync(envPath, 0o600); } catch { /* Windows */ }

console.log(created ? 'Created .env.local from .env.example.' : 'Checked .env.local.');
for (const c of changes) console.log(`  - ${c}`);
if (!changes.length) console.log('  - AUTH_SECRET is already set; left unchanged.');
const missing = ['ADMIN_EMAIL', 'GMAIL_USER', 'GMAIL_APP_PASSWORD'].filter((k) => !get(k));
if (missing.length) console.log(`Still to fill in by hand: ${missing.join(', ')}`);
