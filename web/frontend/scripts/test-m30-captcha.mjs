// M30 Tasks 3 & 4: CAPTCHA Protection & Adaptive Sign-In Security Specification
// Run: node scripts/test-m30-captcha.mjs
import { test, describe, it, after } from 'node:test';
import assert from 'node:assert/strict';
import { transpileForTest } from './transpile-for-test.mjs';

const { load, cleanup, source } = transpileForTest('m30-captcha', {
  dirs: ['lib/server', 'lib', 'components/auth'],
});
after(cleanup);

const {
  verifyCaptchaToken,
  recordLoginFailure,
  isCaptchaRequiredOnLogin,
  clearLoginFailures,
  GENERIC_AUTH_ERROR,
  TURNSTILE_TEST_PASS_TOKEN,
  TURNSTILE_TEST_FAIL_TOKEN,
} = load('lib/server/captcha.js');

const { buildCsp } = load('lib/csp.js');

describe('M30 Tasks 3 & 4: CAPTCHA on Sign-up and Sign-in', () => {

  describe('Task 3: CAPTCHA on Sign-up & Token Verification', () => {
    it('refuses requests with missing token', async () => {
      const emptyRes = await verifyCaptchaToken('', 'signup');
      assert.strictEqual(emptyRes.ok, false);
      assert.strictEqual(emptyRes.reason, 'missing');

      const nullRes = await verifyCaptchaToken(null, 'signup');
      assert.strictEqual(nullRes.ok, false);
      assert.strictEqual(nullRes.reason, 'missing');

      const undefinedRes = await verifyCaptchaToken(undefined, 'signup');
      assert.strictEqual(undefinedRes.ok, false);
      assert.strictEqual(undefinedRes.reason, 'missing');
    });

    it('refuses invalid or malformed tokens', async () => {
      const invalidRes = await verifyCaptchaToken(TURNSTILE_TEST_FAIL_TOKEN, 'signup');
      assert.strictEqual(invalidRes.ok, false);
      assert.strictEqual(invalidRes.reason, 'invalid');
    });

    it('enforces single-use semantics and rejects replay of tokens', async () => {
      const token = 'test_token_single_use_' + Math.random().toString(36);
      
      // First verification: success
      const first = await verifyCaptchaToken(token, 'signup');
      assert.strictEqual(first.ok, true);

      // Second verification with identical token (replay attempt): refused
      const replay = await verifyCaptchaToken(token, 'signup');
      assert.strictEqual(replay.ok, false);
      assert.strictEqual(replay.reason, 'replay', 'Replayed token must be refused');
    });

    it('enforces token expiration after 5-minute TTL', async () => {
      const expiredTimestamp = Date.now() - (6 * 60 * 1000); // 6 minutes ago
      const expiredToken = `tk:signup:${expiredTimestamp}:nonce123`;
      
      const res = await verifyCaptchaToken(expiredToken, 'signup', { issuedAt: expiredTimestamp });
      assert.strictEqual(res.ok, false);
      assert.strictEqual(res.reason, 'expired', 'Expired token must be rejected');
    });

    it('strictly binds tokens to action (sign-up token cannot be used on sign-in or reset)', async () => {
      const signupToken = 'tk:signup:' + Date.now() + ':nonce_abc';

      // Verify sign-up token against sign-in action: must fail
      const loginAttempt = await verifyCaptchaToken(signupToken, 'login', { tokenAction: 'signup' });
      assert.strictEqual(loginAttempt.ok, false);
      assert.strictEqual(loginAttempt.reason, 'action_mismatch', 'Sign-up token cannot be used for login');

      // Verify sign-up token against reset action: must fail
      const resetAttempt = await verifyCaptchaToken(signupToken, 'reset', { tokenAction: 'signup' });
      assert.strictEqual(resetAttempt.ok, false);
      assert.strictEqual(resetAttempt.reason, 'action_mismatch', 'Sign-up token cannot be used for reset');

      // Verify sign-up token against correct signup action: succeeds
      const validSignup = await verifyCaptchaToken(signupToken, 'signup', { tokenAction: 'signup' });
      assert.strictEqual(validSignup.ok, true);
    });

    it('gates test mode so it cannot be enabled in production', async () => {
      const prevEnv = process.env.NODE_ENV;
      try {
        process.env.NODE_ENV = 'production';
        const res = await verifyCaptchaToken(TURNSTILE_TEST_PASS_TOKEN, 'signup');
        assert.strictEqual(res.ok, false, 'Test mode token must be rejected in production');
        assert.strictEqual(res.reason, 'test_mode_rejected_in_prod');
      } finally {
        process.env.NODE_ENV = prevEnv;
      }
    });

    it('fails closed on sign-up during CAPTCHA provider outage (Decision 3)', async () => {
      const res = await verifyCaptchaToken('any_token_sample', 'signup', { mockProviderStatus: 'outage' });
      assert.strictEqual(res.ok, false, 'Sign-up must fail closed during provider outage');
      assert.strictEqual(res.reason, 'outage_fail_closed');
    });
  });

  describe('Task 4: Adaptive CAPTCHA on Sign-in & Enumeration Protection', () => {
    const testEmail = 'trader@spectralabs.io';
    const testIp = '198.51.100.42';

    it('counts failures per account and per IP and requires CAPTCHA after threshold', () => {
      clearLoginFailures(testEmail, testIp);
      assert.strictEqual(isCaptchaRequiredOnLogin(testEmail, testIp), false, 'Initially not required');

      // Failure 1
      recordLoginFailure(testEmail, testIp);
      assert.strictEqual(isCaptchaRequiredOnLogin(testEmail, testIp), false);

      // Failure 2
      recordLoginFailure(testEmail, testIp);
      assert.strictEqual(isCaptchaRequiredOnLogin(testEmail, testIp), false);

      // Failure 3: reaches threshold of 3 failures for account
      recordLoginFailure(testEmail, testIp);
      assert.strictEqual(isCaptchaRequiredOnLogin(testEmail, testIp), true, 'CAPTCHA required after 3 account failures');

      // Clearing failures on successful login resets requirement
      clearLoginFailures(testEmail, testIp);
      assert.strictEqual(isCaptchaRequiredOnLogin(testEmail, testIp), false, 'Requirement cleared after successful login');
    });

    it('triggers CAPTCHA when IP failure threshold is reached across different accounts', () => {
      const attackIp = '203.0.113.99';
      clearLoginFailures('victim1@spectralabs.io', attackIp);
      clearLoginFailures('victim2@spectralabs.io', attackIp);
      clearLoginFailures('victim3@spectralabs.io', attackIp);

      for (let i = 1; i <= 5; i++) {
        recordLoginFailure(`user${i}@spectralabs.io`, attackIp);
      }

      // Any attempt from attackIp now requires CAPTCHA
      assert.strictEqual(isCaptchaRequiredOnLogin('unknown@spectralabs.io', attackIp), true, 'CAPTCHA required after 5 IP failures');
      clearLoginFailures('unknown@spectralabs.io', attackIp);
    });

    it('guarantees error messages do not reveal whether an email exists', () => {
      // Must use generic message
      assert.strictEqual(GENERIC_AUTH_ERROR, 'Incorrect email or password. Please try again.');
      assert.strictEqual(GENERIC_AUTH_ERROR.includes('No account found'), false);
      assert.strictEqual(GENERIC_AUTH_ERROR.includes('does not exist'), false);
    });

    it('fails open on sign-in during provider outage while maintaining rate limits (Decision 3)', async () => {
      const res = await verifyCaptchaToken('sample_token', 'login', { mockProviderStatus: 'outage' });
      assert.strictEqual(res.ok, true, 'Sign-in must fail open during provider outage');
      assert.strictEqual(res.reason, 'outage_fail_open');
    });
  });

  describe('CSP & Accessibility Verification', () => {
    it('keeps CSP changes minimal and enables Turnstile only where specified', () => {
      const nonce = 'abcdef1234567890abcdef==';
      
      // Default / standard route CSP
      const standardCsp = buildCsp({ nonce, dev: false, https: true, captcha: false });
      assert.ok(standardCsp.includes("frame-src 'none'"), 'Standard CSP completely blocks frames');
      assert.strictEqual(standardCsp.includes('challenges.cloudflare.com'), false, 'Standard CSP does not include third-party scripts');

      // Authentication route CSP with CAPTCHA enabled
      const authCsp = buildCsp({ nonce, dev: false, https: true, captcha: true });
      assert.ok(authCsp.includes('https://challenges.cloudflare.com'), 'Auth CSP allowlists Turnstile origin');
      assert.ok(authCsp.includes("frame-src https://challenges.cloudflare.com"), 'Auth CSP permits Turnstile frame');
      assert.ok(authCsp.includes("'strict-dynamic'"), 'Maintains strict-dynamic execution');
      assert.strictEqual(authCsp.includes("'unsafe-inline'"), true, 'Styles remain safely inline');
    });

    it('asserts CaptchaWidget includes accessible screen-reader annotations and responsiveness', () => {
      const widgetSource = source('components/auth/CaptchaWidget.tsx');
      assert.ok(widgetSource.includes('role="region"'), 'Widget declares region role');
      assert.ok(widgetSource.includes('aria-label='), 'Widget provides aria-label');
      assert.ok(widgetSource.includes('max-w-[300px]'), 'Widget fits phone width constraints (300px)');
      assert.ok(widgetSource.includes('sr-only'), 'Widget provides screen-reader announcement text');
    });
  });

});
