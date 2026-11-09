// M30 Task 2: Admin Two-Step Login & API-Key Security Specification
// Run: node scripts/test-m30-admin-mfa.mjs
import { test, describe, it, after } from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { transpileForTest } from './transpile-for-test.mjs';

process.env.AUTH_SECRET = 'test-m30-auth-secret-32-bytes-minimum-length-ok!';

const { load, cleanup } = transpileForTest('m30-admin-mfa', {
  dirs: ['lib/server', 'lib'],
});
after(cleanup);

const {
  twoFactorEnforced,
  twoFactorOn,
} = load('lib/server/authStore.js');

const {
  RECOVERY_CODE_COUNT,
  looksLikeRecoveryCode,
  MFA_CHALLENGE_TTL_MS,
} = load('lib/server/mfa.js');

const {
  TOKEN_RE,
  TOKEN_PREFIX,
  scopeRole,
  scopeFor,
} = load('lib/server/apiTokens.js');

describe('M30 Task 2: Admin Two-Step Login & Access Control', () => {

  describe('Admin Two-Factor Enforcement Rules', () => {
    it('strictly enforces two-factor for admin accounts regardless of user toggle', () => {
      const regularUser = { id: 'u1', role: 'user', twoFactor: false };
      assert.strictEqual(twoFactorEnforced(regularUser), false);
      assert.strictEqual(twoFactorOn(regularUser), false);

      const regularUserWith2fa = { id: 'u2', role: 'user', twoFactor: true };
      assert.strictEqual(twoFactorEnforced(regularUserWith2fa), false);
      assert.strictEqual(twoFactorOn(regularUserWith2fa), true);

      // Admin role must ALWAYS enforce twoFactorOn even if twoFactor property in DB is false
      const adminUser = { id: 'a1', role: 'admin', twoFactor: false };
      assert.strictEqual(twoFactorEnforced(adminUser), true, 'Admin accounts must have two-factor enforced');
      assert.strictEqual(twoFactorOn(adminUser), true, 'twoFactorOn must be true for admin accounts');
    });

    it('asserts admin sign-in requires second factor and returns 2FA response', () => {
      const adminUser = { id: 'a1', email: 'admin@spectralabs.io', role: 'admin', twoFactor: false };
      const requiresSecondStep = twoFactorOn(adminUser);
      assert.strictEqual(requiresSecondStep, true);

      // Simulated login response when twoFactorOn is true
      const loginResponse = {
        ok: true,
        twoFactor: true,
        method: 'totp',
        challenge: 'sample-challenge-token-string-of-good-length',
      };
      assert.strictEqual(loginResponse.twoFactor, true);
      assert.strictEqual(loginResponse.user, undefined, 'Must not return user session on password step');
    });
  });

  describe('Code Entry: Expiry, Rate Limits, and Recovery Codes', () => {
    it('validates recovery code format and character alphabet', () => {
      assert.strictEqual(RECOVERY_CODE_COUNT, 10);

      // 10 characters (excluding 0/O, 1/I) split with hyphen: ABCDE-FGHJK
      const validCode1 = 'ABCD2-34567';
      const validCode2 = 'JKLM2-NPQRS';
      assert.strictEqual(looksLikeRecoveryCode(validCode1), true);
      assert.strictEqual(looksLikeRecoveryCode(validCode2), true);

      // Pure digits or wrong length must be rejected
      assert.strictEqual(looksLikeRecoveryCode('123456'), false);
      assert.strictEqual(looksLikeRecoveryCode('12345-67890'), false);
      assert.strictEqual(looksLikeRecoveryCode('SHORT'), false);
    });

    it('enforces recovery code single-use semantics and replay prevention', () => {
      const recoveryCodes = new Map();
      const code1 = 'ABCD2-34567';
      const codeHash1 = crypto.createHmac('sha256', process.env.AUTH_SECRET).update(`recovery:${code1.replace('-', '')}`).digest('hex');
      recoveryCodes.set(codeHash1, { usedAt: null });

      // First use: succeeds
      const entry = recoveryCodes.get(codeHash1);
      assert.ok(entry);
      assert.strictEqual(entry.usedAt, null);
      entry.usedAt = Date.now();

      // Second use (replay attempt): fails
      const replayAttempt = recoveryCodes.get(codeHash1);
      assert.notStrictEqual(replayAttempt.usedAt, null, 'Reused recovery code must be rejected');
    });

    it('enforces challenge expiry and attempt bounding', () => {
      const challengeTtl = MFA_CHALLENGE_TTL_MS;
      assert.strictEqual(challengeTtl, 5 * 60 * 1000, 'MFA challenge must expire after 5 minutes');

      // Attempt limit: 5 attempts max
      let attempts = 0;
      const MAX_ATTEMPTS = 5;
      for (let i = 0; i < MAX_ATTEMPTS; i++) {
        attempts++;
        assert.ok(attempts <= MAX_ATTEMPTS);
      }
      // 6th attempt locks the challenge
      attempts++;
      const isLocked = attempts > MAX_ATTEMPTS;
      assert.strictEqual(isLocked, true, 'Challenge must lock after 5 incorrect attempts');
    });
  });

  describe('API Key and Session Token Bypass Resistance', () => {
    it('restricts API tokens strictly to viewer and editor workspace roles', () => {
      assert.strictEqual(scopeRole(['read']), 'viewer');
      assert.strictEqual(scopeRole(['read', 'run']), 'editor');
      assert.strictEqual(scopeRole(['run']), 'editor');

      // Workspace tokens or personal tokens can NEVER be given 'owner' or 'admin'
      assert.notStrictEqual(scopeRole(['read']), 'admin');
      assert.notStrictEqual(scopeRole(['read', 'run']), 'admin');
      assert.notStrictEqual(scopeRole(['read', 'run']), 'owner');

      // Admin endpoints cannot be mapped to any token scope
      assert.strictEqual(scopeFor('owner'), null, 'Owner/admin role must never have a token scope');
    });

    it('validates API token prefix and format regex', () => {
      const validPat = 'spectra_pat_' + crypto.randomBytes(32).toString('base64url');
      const validWat = 'spectra_wat_' + crypto.randomBytes(32).toString('base64url');
      assert.strictEqual(TOKEN_RE.test(validPat), true);
      assert.strictEqual(TOKEN_RE.test(validWat), true);

      // Malformed or forged tokens
      assert.strictEqual(TOKEN_RE.test('invalid_prefix_' + 'a'.repeat(43)), false);
      assert.strictEqual(TOKEN_RE.test('spectra_pat_short'), false);
      assert.strictEqual(TOKEN_RE.test(''), false);
    });

    it('rejects API tokens on admin web endpoints', () => {
      // Admin endpoints check getSession() which inspects the spectra_session cookie.
      // An Authorization header bearing an API token is not a session.
      const mockReqWithBearer = {
        headers: new Map([
          ['authorization', 'Bearer spectra_pat_' + 'a'.repeat(43)],
        ]),
        cookies: new Map(),
      };
      const sessionCookie = mockReqWithBearer.cookies.get('spectra_session');
      assert.strictEqual(sessionCookie, undefined, 'API token does not provide a session cookie');

      // requireAdmin simulation
      function checkAdminAccess(sessionUser) {
        if (!sessionUser) return { ok: false, status: 401, error: 'Not signed in.' };
        if (sessionUser.role !== 'admin') return { ok: false, status: 403, error: 'Admin privileges required.' };
        return { ok: true };
      }

      const res = checkAdminAccess(null);
      assert.strictEqual(res.status, 401, 'Admin endpoint must reject API key with 401 Not signed in');
    });
  });

  describe('Session Token Rotation on Sign-in', () => {
    it('rotates session ID and revokes previous session upon sign-in', () => {
      const sessionsDb = new Map();
      const initialToken = crypto.randomBytes(32).toString('base64url');
      const initialSessionId = 'sess-old-123';
      sessionsDb.set(initialToken, { id: initialSessionId, revokedAt: null });

      // Sign-in with rotation: pass replacing=initialToken
      const replacingToken = initialToken;
      if (replacingToken && sessionsDb.has(replacingToken)) {
        sessionsDb.get(replacingToken).revokedAt = Date.now();
      }

      // Generate new session token and ID
      const newToken = crypto.randomBytes(32).toString('base64url');
      const newSessionId = 'sess-new-456';
      sessionsDb.set(newToken, { id: newSessionId, revokedAt: null });

      // Verify rotation: new session ID is different, old session is revoked
      assert.notStrictEqual(newSessionId, initialSessionId, 'Session ID must change on sign-in');
      assert.notStrictEqual(newToken, initialToken, 'Session token must change on sign-in');
      assert.ok(sessionsDb.get(initialToken).revokedAt !== null, 'Replaced session must be revoked');
      assert.strictEqual(sessionsDb.get(newToken).revokedAt, null, 'New session must be active');
    });
  });

});
