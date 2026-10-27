// Shared helper for the node:test suites: transpiles the given TypeScript files (and everything
// under the given directories) to CommonJS in a cache folder, keeping the directory layout so
// relative requires resolve. Type-only imports are erased by TypeScript, so '@/...' type imports
// need no alias resolution. Returns a loader and a cleanup function.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);
const ts = require(path.join(root, 'node_modules/typescript'));

export function transpileForTest(name, { files = [], dirs = [] } = {}) {
  const build = path.join(root, 'node_modules', '.cache', `spectra-${name}-test`);
  fs.rmSync(build, { recursive: true, force: true });
  fs.mkdirSync(build, { recursive: true });
  fs.writeFileSync(path.join(build, 'package.json'), '{"type":"commonjs"}');
  const walk = (dir) => fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(dir, e.name)) : [path.join(dir, e.name)]));
  const sources = [...files.map((f) => path.join(root, f)), ...dirs.flatMap((d) => walk(path.join(root, d)))].filter((f) => f.endsWith('.ts'));
  for (const file of sources) {
    const { outputText } = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
      compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, esModuleInterop: true },
    });
    const out = path.join(build, path.relative(root, file).replace(/\.ts$/, '.js'));
    fs.mkdirSync(path.dirname(out), { recursive: true });
    fs.writeFileSync(out, outputText);
  }
  return {
    load: (rel) => require(path.join(build, rel)),
    source: (rel) => fs.readFileSync(path.join(root, rel), 'utf8'),
    cleanup: () => fs.rmSync(build, { recursive: true, force: true }),
  };
}
