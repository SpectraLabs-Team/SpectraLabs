// Prints lib/server/auth/sql.ts as psql PREPARE statements, so CI can check them against the schema
// (crates/spectra-db/tests/auth_sql_checks.sql). Usage: node scripts/print-auth-sql.mjs [path/to/sql.ts]
import fs from 'node:fs';
const src = fs.readFileSync(process.argv[2] || new URL('../lib/server/auth/sql.ts', import.meta.url), 'utf8');
const consts = {};
for (const m of src.matchAll(/const (\w+) = `([\s\S]*?)`;/g)) consts[m[1]] = m[2];
const expand = (s) => s.replace(/\$\{(\w+)\}/g, (_, k) => consts[k]);
const types = { FIND_USER: 'text', LIST_USERS: '', INSERT_USER: 'text,text,text,text,boolean', UPDATE_USER: 'text,text,float8,boolean',
  PUT_CODE: 'text,text,text,text,text,boolean,float8', LOCK_CODE: 'text,text', PEEK_CODE: 'text,text', DELETE_CODE: 'text,text',
  BUMP_ATTEMPTS: 'text,text', PURGE_EXPIRED_CODES: '' };
for (const [k, t] of Object.entries(types)) {
  if (!consts[k]) throw new Error('missing ' + k);
  console.log(`PREPARE ${k.toLowerCase()}${t ? '(' + t + ')' : ''} AS ${expand(consts[k])};`);
}
