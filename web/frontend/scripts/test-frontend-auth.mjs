// Unit & integration tests for Frontend Auth, Session Edge & Route Guards (M19 Tasks 1–4, 14, 18).
// Run: node scripts/test-frontend-auth.mjs
import { test, describe, it } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const root = path.resolve(__dirname, '..');
const require = createRequire(import.meta.url);

const ts = require(path.join(root, 'node_modules/typescript'));

function transpileAndLoad(relPath) {
  const tsSource = fs.readFileSync(path.join(root, relPath), 'utf8');
  const { outputText } = ts.transpileModule(tsSource, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
  });
  const tempDir = path.join(root, 'node_modules', '.cache', 'auth-fe-test');
  fs.mkdirSync(tempDir, { recursive: true });
  const tempFile = path.join(tempDir, path.basename(relPath).replace(/\.tsx?$/, '.cjs'));
  fs.writeFileSync(tempFile, outputText);
  return require(tempFile);
}

const {
  MIN_PASSWORD,
  MAX_PASSWORD,
  EMAIL_RE,
  checkPasswordRules,
  isPasswordValid,
  cleanName,
  nameFromEmail,
} = transpileAndLoad('lib/auth-shared.ts');

const {
  SESSION_TOKEN_RE,
  looksLikeSessionToken,
} = transpileAndLoad('lib/sessionEdge.ts');

const {
  buildCsp,
  cspMode,
  newNonce,
  isCrossSite,
  allowedOrigins,
} = transpileAndLoad('lib/csp.ts');

describe('M19 Frontend Auth & Route Protection Specification', () => {

  describe('Password Checklist & Credentials Rules (M19 Task 1)', () => {
    it('enforces password criteria: 8+ chars, letter, number, and special character', () => {
      // Missing length
      const short = checkPasswordRules('A1!b');
      assert.strictEqual(short.length, false);
      assert.strictEqual(short.letter, true);
      assert.strictEqual(short.number, true);
      assert.strictEqual(short.special, true);
      assert.strictEqual(isPasswordValid('A1!b'), false);

      // Missing letter
      const noLetter = checkPasswordRules('1234567!9');
      assert.strictEqual(noLetter.length, true);
      assert.strictEqual(noLetter.letter, false);
      assert.strictEqual(isPasswordValid('1234567!9'), false);

      // Missing number
      const noNumber = checkPasswordRules('Password!@#');
      assert.strictEqual(noNumber.number, false);
      assert.strictEqual(isPasswordValid('Password!@#'), false);

      // Missing special character
      const noSpecial = checkPasswordRules('Password123');
      assert.strictEqual(noSpecial.special, false);
      assert.strictEqual(isPasswordValid('Password123'), false);

      // Fully valid password
      const valid = checkPasswordRules('SecureP@ssw0rd!');
      assert.strictEqual(valid.length, true);
      assert.strictEqual(valid.letter, true);
      assert.strictEqual(valid.number, true);
      assert.strictEqual(valid.special, true);
      assert.strictEqual(isPasswordValid('SecureP@ssw0rd!'), true);
    });

    it('enforces maximum password length bound to prevent DoS on hashing', () => {
      const hugePassword = 'A1!' + 'a'.repeat(MAX_PASSWORD);
      assert.strictEqual(isPasswordValid(hugePassword), false, 'Passwords > 256 chars must be rejected');
    });

    it('validates email formats with RFC-compliant checks', () => {
      assert.strictEqual(EMAIL_RE.test('user@spectralabs.io'), true);
      assert.strictEqual(EMAIL_RE.test('quant.trader+v1@firm.co.uk'), true);
      assert.strictEqual(EMAIL_RE.test('invalid-email'), false);
      assert.strictEqual(EMAIL_RE.test('@missinguser.com'), false);
      assert.strictEqual(EMAIL_RE.test('spaces in@email.com'), false);
    });

    it('sanitizes and normalizes user display names', () => {
      assert.strictEqual(cleanName('   Alice   Trader   '), 'Alice Trader');
      assert.strictEqual(cleanName('A'), '', 'Single-letter names must fail minimum length');
      assert.strictEqual(cleanName('a'.repeat(81)), '', 'Names over 80 chars must be rejected');
      assert.strictEqual(nameFromEmail('john.doe+work@domain.com'), 'John Doe Work');
    });
  });

  describe('Session Token Format & Edge Validation (M19 Task 2)', () => {
    it('accepts valid 43-character base64url random session tokens', () => {
      // 32 random bytes in base64url produces 43 characters without padding
      const validToken = 'session_mock_token_'.padEnd(43, '0');
      assert.strictEqual(validToken.length, 43);
      assert.strictEqual(SESSION_TOKEN_RE.test(validToken), true);
      assert.strictEqual(looksLikeSessionToken(validToken), true);
    });

    it('rejects malformed, corrupted, or non-conforming tokens at the edge', () => {
      assert.strictEqual(looksLikeSessionToken(''), false);
      assert.strictEqual(looksLikeSessionToken(null), false);
      assert.strictEqual(looksLikeSessionToken(undefined), false);
      assert.strictEqual(looksLikeSessionToken('short-token'), false);
      // Rejects tokens containing invalid base64url characters (+, /, =)
      assert.strictEqual(looksLikeSessionToken('mock_invalid_token_with_symbols_+/_====1234'), false);
      // Rejects JSON stringified tokens from old implementations
      assert.strictEqual(looksLikeSessionToken('{"userId":"123","email":"test@test.com"}'), false);
    });
  });

  describe('Route Protection & Edge Middleware Gating (M19 Task 3 & 4)', () => {
    const PROTECTED_ROUTES = ['/studio', '/admin', '/account'];
    const isProtected = (p) => PROTECTED_ROUTES.some((base) => p === base || p.startsWith(`${base}/`));

    it('correctly identifies protected studio, admin, and account paths', () => {
      assert.strictEqual(isProtected('/studio'), true);
      assert.strictEqual(isProtected('/studio/backtests'), true);
      assert.strictEqual(isProtected('/studio/experiments'), true);
      assert.strictEqual(isProtected('/admin'), true);
      assert.strictEqual(isProtected('/admin/users'), true);
      assert.strictEqual(isProtected('/account'), true);
      assert.strictEqual(isProtected('/account/security'), true);

      // Public routes
      assert.strictEqual(isProtected('/'), false);
      assert.strictEqual(isProtected('/pricing'), false);
      assert.strictEqual(isProtected('/docs'), false);
      assert.strictEqual(isProtected('/api/health'), false);
    });

    it('simulates middleware session redirection for unauthenticated visitors', () => {
      function evaluateMiddleware(pathname, cookieValue) {
        if (!isProtected(pathname)) return { action: 'pass' };
        if (!looksLikeSessionToken(cookieValue)) {
          return {
            action: 'redirect',
            target: `/?auth=login&next=${encodeURIComponent(pathname)}`,
            clearCookie: !!cookieValue,
          };
        }
        return { action: 'pass' };
      }

      // Case 1: Visitor visits /studio with no cookie
      const noAuth = evaluateMiddleware('/studio/backtests', undefined);
      assert.strictEqual(noAuth.action, 'redirect');
      assert.strictEqual(noAuth.target, '/?auth=login&next=%2Fstudio%2Fbacktests');
      assert.strictEqual(noAuth.clearCookie, false);

      // Case 2: Visitor visits with corrupted/old cookie
      const corruptedAuth = evaluateMiddleware('/admin', 'invalid_cookie');
      assert.strictEqual(corruptedAuth.action, 'redirect');
      assert.strictEqual(corruptedAuth.clearCookie, true, 'Corrupted cookie must be cleared');

      // Case 3: Authenticated visitor with valid token
      const validToken = 'session_mock_token_'.padEnd(43, '0');
      const authed = evaluateMiddleware('/studio', validToken);
      assert.strictEqual(authed.action, 'pass');
    });
  });

  describe('CSRF Guard & Cross-Site Protection (M19 Task 14)', () => {
    it('permits idempotent read methods (GET, HEAD, OPTIONS) regardless of origin', () => {
      const getReq = { method: 'GET', origin: 'https://malicious.com', host: 'spectra.app', secFetchSite: 'cross-site' };
      assert.strictEqual(isCrossSite(getReq), false);
    });

    it('blocks cross-origin state mutations (POST, PUT, DELETE, PATCH)', () => {
      const postFromAttacker = {
        method: 'POST',
        origin: 'https://attacker.site',
        host: 'spectra.app',
        secFetchSite: 'cross-site',
      };
      assert.strictEqual(isCrossSite(postFromAttacker, ['https://spectra.app']), true);
    });

    it('permits same-origin and allowlisted mutations', () => {
      const sameOriginPost = {
        method: 'POST',
        origin: 'https://spectra.app',
        host: 'spectra.app',
        secFetchSite: 'same-origin',
      };
      assert.strictEqual(isCrossSite(sameOriginPost, ['https://spectra.app']), false);

      const allowlistedPost = {
        method: 'POST',
        origin: 'https://admin.spectra.app',
        host: 'api.spectra.app',
        secFetchSite: 'cross-site',
      };
      assert.strictEqual(isCrossSite(allowlistedPost, ['https://admin.spectra.app', 'https://api.spectra.app']), false);
    });

    it('generates secure Content-Security-Policy with strict nonce and no clickjacking', () => {
      const nonce = newNonce();
      assert.ok(typeof nonce === 'string' && nonce.length > 10);
      const csp = buildCsp({ nonce, dev: false, https: true });

      assert.ok(csp.includes(`'nonce-${nonce}'`), 'CSP must contain random per-request nonce');
      assert.ok(csp.includes("'strict-dynamic'"), 'CSP must use strict-dynamic for safe script chunk loading');
      assert.ok(csp.includes("frame-ancestors 'none'"), 'CSP must prevent framing/clickjacking');
      assert.ok(csp.includes("base-uri 'self'"), 'CSP must lock base-uri');
    });
  });

  describe('Workspace Role Hierarchy & Permission Rules (M19 Task 4)', () => {
    const ROLE_RANK = { viewer: 10, editor: 20, owner: 30 };
    const hasRole = (role, minRole) => ROLE_RANK[role] >= ROLE_RANK[minRole];

    it('strictly enforces role ranking: viewer < editor < owner', () => {
      // Viewer tests
      assert.strictEqual(hasRole('viewer', 'viewer'), true);
      assert.strictEqual(hasRole('viewer', 'editor'), false, 'Viewer cannot execute editor actions');
      assert.strictEqual(hasRole('viewer', 'owner'), false, 'Viewer cannot execute owner actions');

      // Editor tests
      assert.strictEqual(hasRole('editor', 'viewer'), true);
      assert.strictEqual(hasRole('editor', 'editor'), true);
      assert.strictEqual(hasRole('editor', 'owner'), false, 'Editor cannot execute owner actions');

      // Owner tests
      assert.strictEqual(hasRole('owner', 'viewer'), true);
      assert.strictEqual(hasRole('owner', 'editor'), true);
      assert.strictEqual(hasRole('owner', 'owner'), true);
    });
  });
});
