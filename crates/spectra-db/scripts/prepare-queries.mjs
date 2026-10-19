// Prints every queries/*.sql as a psql PREPARE statement (types from its "-- params:" line), so
// tests/queue_checks.sql can EXECUTE exactly the SQL the Rust code runs.
//   node crates/spectra-db/scripts/prepare-queries.mjs > /tmp/prep.sql
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'queries');
for (const f of fs.readdirSync(dir).filter((f) => f.endsWith('.sql')).sort()) {
  const text = fs.readFileSync(path.join(dir, f), 'utf8');
  const params = (text.match(/^-- params:(.*)$/m) || [, ''])[1].trim();
  const body = text.split('\n').filter((l) => !l.trimStart().startsWith('--')).join('\n').trim();
  console.log(`PREPARE ${f.replace(/\.sql$/, '')}${params ? `(${params})` : ''} AS\n${body};\n`);
}
