// Re-wrap every workspace data key with the current master key (M19 task 8).
//
//   1. Generate a new key:  node -e "console.log(require('crypto').randomBytes(32).toString('base64'))"
//   2. Put it FIRST in SECRETS_MASTER_KEY, keeping the old one:  SECRETS_MASTER_KEY=2:<new>,1:<old>
//      (a plain "<old>" key counts as version 1). Restart the app.
//   3. npm run secrets:rotate        → re-wraps every workspace key under version 2
//   4. When it reports nothing left, drop the old entry:  SECRETS_MASTER_KEY=2:<new>
//
// Stored secrets aren't re-encrypted (their data keys don't change); only the small wrapped keys are.
// Uses the app's own vault code (lib/server/vault.ts), compiled on the fly.
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { loadEnv, root } from './load-env.mjs';

loadEnv();
const require = createRequire(import.meta.url);
const ts = require(path.join(root, 'node_modules/typescript'));
const build = path.join(root, 'node_modules', '.cache', 'spectra-secrets-rotate');
fs.mkdirSync(path.join(build, 'lib/server'), { recursive: true });
fs.writeFileSync(path.join(build, 'package.json'), '{"type":"commonjs"}');
for (const rel of ['lib/server/db.ts', 'lib/server/audit.ts', 'lib/server/vault.ts']) {
  const { outputText } = ts.transpileModule(fs.readFileSync(path.join(root, rel), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, esModuleInterop: true },
  });
  fs.writeFileSync(path.join(build, rel.replace(/\.ts$/, '.js')), outputText);
}
const db = require(path.join(build, 'lib/server/db.js'));
const vault = require(path.join(build, 'lib/server/vault.js'));

try {
  const ring = vault.parseKeyring(process.env.SECRETS_MASTER_KEY);
  console.log(`Current master key: version ${ring.current.version}. Known versions: ${[...ring.byVersion.keys()].join(', ')}.`);
  const n = await vault.rotateMasterKey();
  const [{ left }] = await db.query(
    'SELECT (SELECT count(*) FROM workspace_keys WHERE master_version <> $1) + (SELECT count(*) FROM user_totp WHERE master_version <> $1) AS left',
    [ring.current.version],
  );
  console.log(`Re-wrapped ${n} key(s). ${Number(left)} left on older versions.`);
  if (!left) console.log(`Older master keys are no longer needed: set SECRETS_MASTER_KEY=${ring.current.version}:<current key>.`);
} catch (err) {
  console.error('✖', err.message);
  process.exitCode = 1;
} finally {
  await db.closePool();
}
