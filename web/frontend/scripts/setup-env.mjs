// Create or repair web/frontend/.env.local. Run: npm run setup
// - Copies .env.example to .env.local if it doesn't exist.
// - Fills AUTH_SECRET with 48 random bytes when it is empty, too short or the old public default.
// - Fills DATABASE_URL with the local default (Postgres.app / Homebrew) when it is empty.
// - Fills ENGINE_SHARED_SECRET (signs the identity header sent to the engine) when it is empty or short.
// - Fills SECRETS_MASTER_KEY (encrypts stored broker keys) when it is empty. Never replaces one:
//   a different master key would make every stored secret unreadable.
// - Never overwrites a value that is already good, never prints secrets.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const examplePath = path.join(root, '.env.example');
const envPath = path.join(root, '.env.local');
const RETIRED = new Set(['spectralabs_jwt_default_secret_key_32bytes_required']);
const DEFAULT_DATABASE_URL = 'postgresql://localhost:5432/spectra';

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

const engineSecret = get('ENGINE_SHARED_SECRET');
if (!engineSecret || engineSecret.length < 32) {
  set('ENGINE_SHARED_SECRET', crypto.randomBytes(48).toString('base64url'));
  changes.push('generated ENGINE_SHARED_SECRET (give the engine the same value)');
}

if (!get('SECRETS_MASTER_KEY')) {
  set('SECRETS_MASTER_KEY', crypto.randomBytes(32).toString('base64'));
  changes.push('generated SECRETS_MASTER_KEY (back it up: without it, stored broker keys are lost)');
}

if (!get('DATABASE_URL')) {
  set('DATABASE_URL', DEFAULT_DATABASE_URL);
  changes.push(`set DATABASE_URL=${DEFAULT_DATABASE_URL} (change it if your Postgres runs elsewhere)`);
}

fs.writeFileSync(envPath, text, { mode: 0o600 });
try { fs.chmodSync(envPath, 0o600); } catch { /* Windows */ }

console.log(created ? 'Created .env.local from .env.example.' : 'Checked .env.local.');
for (const c of changes) console.log(`  - ${c}`);
if (!changes.length) console.log('  - AUTH_SECRET, ENGINE_SHARED_SECRET, SECRETS_MASTER_KEY and DATABASE_URL are already set; left unchanged.');
const missing = ['ADMIN_EMAIL', 'GMAIL_USER', 'GMAIL_APP_PASSWORD'].filter((k) => !get(k));
if (missing.length) console.log(`Still to fill in by hand: ${missing.join(', ')}`);
