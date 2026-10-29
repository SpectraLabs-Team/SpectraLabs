import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { transpileForTest } from './transpile-for-test.mjs';
import securityHeaders from '../lib/securityHeaders.js';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const frontendDir = path.resolve(__dirname, '..');

const { load } = transpileForTest('csp-hardening', {
  files: ['lib/csp.ts'],
});

const {
  buildCsp,
  cspMode,
  newNonce,
  isCrossSite,
  allowedOrigins,
  CSP_REPORT_PATH,
} = load('lib/csp.js');

test('security and production CSP hardening', async (t) => {
  await t.test('cspMode resolves enforce, report-only, and off correctly', () => {
    assert.equal(cspMode('enforce'), 'enforce', 'enforce mode recognized');
    assert.equal(cspMode('report-only'), 'report-only', 'report-only mode recognized');
    assert.equal(cspMode('off'), 'off', 'off mode recognized');
    assert.equal(cspMode(undefined), 'report-only', 'defaults to report-only when unset');
    assert.equal(cspMode('invalid-mode'), 'report-only', 'falls back to report-only on unknown string');
  });

  await t.test('buildCsp generates hardened nonced policy without unsafe script execution', () => {
    const nonce = newNonce();
    assert.match(nonce, /^[A-Za-z0-9+/]{22}==$/, 'Cryptographically random 128-bit base64 nonce');

    const prodCsp = buildCsp({ nonce, dev: false, https: true });
    const directives = Object.fromEntries(
      prodCsp.split('; ').map((d) => {
        const parts = d.split(' ');
        return [parts[0], parts.slice(1).join(' ')];
      })
    );

    // Strict script protection
    assert.ok(directives['script-src'].includes(`'nonce-${nonce}'`), 'Script-src carries per-request nonce');
    assert.ok(directives['script-src'].includes("'strict-dynamic'"), 'Script-src uses strict-dynamic');
    assert.equal(directives['script-src'].includes("'unsafe-inline'"), false, 'No unsafe-inline for scripts');
    assert.equal(directives['script-src'].includes("'unsafe-eval'"), false, 'No unsafe-eval for scripts in production');

    // Clickjacking & framing protection
    assert.equal(directives['frame-src'], "'none'", 'No frames allowed');
    assert.equal(directives['frame-ancestors'], "'none'", 'Framing is completely blocked');
    assert.equal(directives['object-src'], "'none'", 'Flash/plugins blocked');

    // Base URI injection & Form submission protection
    assert.equal(directives['base-uri'], "'self'");
    assert.equal(directives['form-action'], "'self'");

    // HTTPS upgrade & reporting
    assert.ok(prodCsp.includes('upgrade-insecure-requests'), 'Includes upgrade-insecure-requests when HTTPS');
    assert.ok(directives['report-uri'] === CSP_REPORT_PATH, 'Points to internal CSP violation reporting endpoint');
  });

  await t.test('all inline scripts in app/layout.tsx carry the per-request nonce', () => {
    const layoutPath = path.join(frontendDir, 'app/layout.tsx');
    const content = fs.readFileSync(layoutPath, 'utf8');

    // Read all <script> occurrences
    const scriptMatches = content.match(/<script\b[^>]*>/g) || [];
    assert.ok(scriptMatches.length >= 2, 'Layout has inline scripts (theme & scroll)');
    for (const tag of scriptMatches) {
      assert.ok(tag.includes('nonce={nonce}'), `Tag ${tag} must carry nonce={nonce}`);
    }
  });

  await t.test('no un-nonced inline scripts exist across app or components', () => {
    function scanDir(dir) {
      const entries = fs.readdirSync(dir, { withFileTypes: true });
      for (const entry of entries) {
        const fullPath = path.join(dir, entry.name);
        if (entry.isDirectory()) {
          scanDir(fullPath);
        } else if (entry.isFile() && (entry.name.endsWith('.tsx') || entry.name.endsWith('.jsx'))) {
          if (fullPath.endsWith('layout.tsx')) continue; // layout tested above
          const src = fs.readFileSync(fullPath, 'utf8');
          const inlineScripts = src.match(/<script\b[^>]*>/g) || [];
          assert.equal(
            inlineScripts.length,
            0,
            `File ${fullPath} contains raw inline script tags; all scripts must go through layout.tsx with nonce`
          );
        }
      }
    }

    scanDir(path.join(frontendDir, 'app'));
    scanDir(path.join(frontendDir, 'components'));
  });

  await t.test('securityHeaders module provides hardened baseline HTTP response headers', () => {
    const headers = securityHeaders.pageHeaders({ HSTS: 'on' });
    const headerMap = Object.fromEntries(headers.map((h) => [h.key, h.value]));

    assert.equal(headerMap['X-Frame-Options'], 'DENY');
    assert.equal(headerMap['X-Content-Type-Options'], 'nosniff');
    assert.equal(headerMap['Cross-Origin-Opener-Policy'], 'same-origin');
    assert.equal(headerMap['X-Permitted-Cross-Domain-Policies'], 'none');
    assert.ok(headerMap['Strict-Transport-Security'].includes('max-age=31536000'));
  });

  await t.test('cross-site request guard enforces strict origin verification for state mutations', () => {
    const origins = allowedOrigins({ APP_URL: 'https://app.spectralabs.dev' });
    assert.deepEqual(origins, ['https://app.spectralabs.dev']);

    // Allowed same-origin mutation
    const allowed = isCrossSite(
      { method: 'POST', host: 'app.spectralabs.dev', origin: 'https://app.spectralabs.dev' },
      origins
    );
    assert.equal(allowed, false, 'Same-origin POST is allowed');

    // Blocked cross-site mutation
    const blocked = isCrossSite(
      { method: 'POST', host: 'app.spectralabs.dev', origin: 'https://attacker.com' },
      origins
    );
    assert.equal(blocked, true, 'Cross-origin POST is rejected');
  });
});
