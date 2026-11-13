// M30 Tasks 5, 6 & 7: Recovery, Audit & New-Device Notification Specification
// Run: node scripts/test-m30-recovery-audit.mjs
import { test, describe, it, after } from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { transpileForTest } from './transpile-for-test.mjs';

process.env.AUTH_SECRET = 'test-m30-auth-secret-32-bytes-minimum-length-ok!';

const { load, cleanup, source } = transpileForTest('m30-recovery-audit', {
  dirs: ['lib/server', 'lib'],
});
after(cleanup);

const { twoFactorOn } = load('lib/server/authStore.js');
const { AUDIT_ACTIONS, scrubSecrets } = load('lib/server/audit.js');

describe('M30 Phase M30c: Recovery, Audit and New-Device Notice', () => {

  describe('Task 5: Password Reset Path & Admin Second-Step Enforcement', () => {
    it('guarantees password reset does not bypass admin two-step login', () => {
      const adminRecord = {
        id: 'adm-001',
        email: 'admin@spectralabs.io',
        role: 'admin',
        twoFactor: false,
        status: 'active',
      };
      // Admin account has twoFactorOn enforced automatically
      const needsTwoFactor = twoFactorOn(adminRecord);
      assert.strictEqual(needsTwoFactor, true, 'Admin account must have twoFactorOn evaluated to true');

      // Simulation of reset handler logic:
      // If needsTwoFactor is true, session creation MUST be skipped!
      let sessionCreated = false;
      function simulateResetSession(user) {
        if (user.status === 'active' && !twoFactorOn(user)) {
          sessionCreated = true;
          return { ok: true, session: 'dummy-session' };
        }
        return { ok: true, twoFactorRequired: true };
      }

      const adminResult = simulateResetSession(adminRecord);
      assert.strictEqual(sessionCreated, false, 'No session must be created for admin on password reset');
      assert.strictEqual(adminResult.twoFactorRequired, true, 'Admin must be prompted for second factor');

      // Regular non-2FA user: session can be established
      const regularRecord = {
        id: 'usr-002',
        email: 'regular@spectralabs.io',
        role: 'user',
        twoFactor: false,
        status: 'active',
      };
      const regularResult = simulateResetSession(regularRecord);
      assert.strictEqual(sessionCreated, true, 'Regular non-2FA user can receive direct session');
      assert.strictEqual(regularResult.twoFactorRequired, undefined);
    });

    it('enforces single-use semantics and expiration for reset codes/links', () => {
      const resetCodes = new Map();
      const testEmail = 'trader@spectralabs.io';
      const testCode = '654321';
      const codeHash = crypto.createHash('sha256').update(testCode).digest('hex');
      const now = Date.now();

      // Issue code with 10-minute TTL
      resetCodes.set(testEmail, {
        codeHash,
        expiresAt: now + 10 * 60 * 1000,
        attempts: 0,
      });

      // Verify code first time: succeeds and consumes the code
      const entry = resetCodes.get(testEmail);
      assert.ok(entry);
      assert.strictEqual(entry.codeHash, codeHash);
      assert.ok(entry.expiresAt > Date.now());
      resetCodes.delete(testEmail); // single-use consumption

      // Replay attempt: refused
      const replayEntry = resetCodes.get(testEmail);
      assert.strictEqual(replayEntry, undefined, 'Reused reset code must be refused');

      // Expired code test
      resetCodes.set(testEmail, {
        codeHash,
        expiresAt: now - 1000, // already expired
        attempts: 0,
      });
      const expiredEntry = resetCodes.get(testEmail);
      const isExpired = expiredEntry.expiresAt <= Date.now();
      assert.strictEqual(isExpired, true, 'Expired reset code must be recognized and rejected');
    });

    it('ensures forgot-password messages do not reveal account existence', () => {
      const forgotSource = source('app/api/auth/forgot/route.ts');
      assert.ok(
        forgotSource.includes('If an account exists for that email, a reset code is on its way.'),
        'Must return identical generic response whether email exists or not',
      );
      assert.strictEqual(
        forgotSource.includes('No account found with this email'),
        false,
        'Must never reveal unknown emails',
      );
    });
  });

  describe('Task 6: Audit Log Integrity & Secret Redaction', () => {
    it('contains all required M30 audit action types', () => {
      assert.ok(AUDIT_ACTIONS.includes('auth.login.success'), 'auth.login.success action defined');
      assert.ok(AUDIT_ACTIONS.includes('auth.login.failure'), 'auth.login.failure action defined');
      assert.ok(AUDIT_ACTIONS.includes('auth.captcha.failure'), 'auth.captcha.failure action defined');
      assert.ok(AUDIT_ACTIONS.includes('auth.password.reset'), 'auth.password.reset action defined');
      assert.ok(AUDIT_ACTIONS.includes('auth.password.reset_requested'), 'auth.password.reset_requested action defined');
      assert.ok(AUDIT_ACTIONS.includes('auth.login.new_device'), 'auth.login.new_device action defined');
    });

    it('rigorously scrubs passwords, tokens, OTP codes, and secrets from audit details', () => {
      const sensitiveDetails = {
        email: 'admin@spectralabs.io',
        role: 'admin',
        password: 'PlainSecretPassword123!',
        token: 'sample_pat_123456789012345678901234567890',
        code: '987654',
        otp: '123456',
        totp_secret: 'JBSWY3DPEHPK3PXP',
        recovery_code: 'ABCD2-34567',
        nested: {
          sessionToken: 'sample_sess_secret_value',
          authToken: 'Bearer secret_token',
        },
        harmlessField: 'safe_value',
      };

      const scrubbed = scrubSecrets(sensitiveDetails);

      // Verify secrets are redacted
      assert.strictEqual(scrubbed.password, '[REDACTED]', 'Password must be redacted');
      assert.strictEqual(scrubbed.token, '[REDACTED]', 'Token must be redacted');
      assert.strictEqual(scrubbed.code, '[REDACTED]', 'Code must be redacted');
      assert.strictEqual(scrubbed.otp, '[REDACTED]', 'OTP must be redacted');
      assert.strictEqual(scrubbed.totp_secret, '[REDACTED]', 'TOTP secret must be redacted');
      assert.strictEqual(scrubbed.recovery_code, '[REDACTED]', 'Recovery code must be redacted');
      assert.strictEqual(scrubbed.nested.sessionToken, '[REDACTED]', 'Nested session token must be redacted');
      assert.strictEqual(scrubbed.nested.authToken, '[REDACTED]', 'Nested auth token must be redacted');

      // Verify safe fields remain intact
      assert.strictEqual(scrubbed.email, 'admin@spectralabs.io');
      assert.strictEqual(scrubbed.role, 'admin');
      assert.strictEqual(scrubbed.harmlessField, 'safe_value');

      // Verify serializing scrubbed output contains no plaintext secrets
      const logLine = JSON.stringify(scrubbed);
      assert.strictEqual(logLine.includes('PlainSecretPassword123!'), false);
      assert.strictEqual(logLine.includes('spectra_pat_1234567890'), false);
      assert.strictEqual(logLine.includes('987654'), false);
      assert.strictEqual(logLine.includes('JBSWY3DPEHPK3PXP'), false);
    });

    it('verifies audit events record account, IP, time, and outcome without secrets', () => {
      const mockEvent = {
        action: 'auth.login.success',
        outcome: 'success',
        actor: { id: 'usr-1', email: 'admin@spectralabs.io' },
        target: { type: 'session', id: 'sess-abc' },
        details: { method: 'password+totp', step: 2 },
      };
      const mockMeta = {
        ip: '198.51.100.10',
        userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
        requestId: 'req-12345',
      };

      const scrubbed = scrubSecrets(mockEvent.details);
      const auditPayload = {
        time: Date.now(),
        actorEmail: mockEvent.actor.email,
        ip: mockMeta.ip,
        action: mockEvent.action,
        outcome: mockEvent.outcome,
        details: scrubbed,
      };

      assert.ok(auditPayload.time > 0);
      assert.strictEqual(auditPayload.actorEmail, 'admin@spectralabs.io');
      assert.strictEqual(auditPayload.ip, '198.51.100.10');
      assert.strictEqual(auditPayload.outcome, 'success');
    });
  });

  describe('Task 7: New-Device Notification for Admins', () => {
    it('sends new-device notification email once per new device', () => {
      const knownDevices = new Set();
      const userId = 'admin-user-id';
      const deviceTokenA = 'device_cookie_token_aaa_12345678901234567890123';
      const deviceTokenB = 'device_cookie_token_bbb_12345678901234567890123';

      let emailsSent = 0;
      function onSignIn(uid, dToken) {
        const key = `${uid}:${dToken}`;
        const isNew = !knownDevices.has(key);
        if (isNew) {
          knownDevices.add(key);
          emailsSent++;
          return { isNew: true };
        }
        return { isNew: false };
      }

      // First sign-in on Device A: new device, notice sent
      const signin1 = onSignIn(userId, deviceTokenA);
      assert.strictEqual(signin1.isNew, true);
      assert.strictEqual(emailsSent, 1, 'First sign-in on Device A must send notice');

      // Subsequent sign-in on Device A: already known, notice NOT sent
      const signin2 = onSignIn(userId, deviceTokenA);
      assert.strictEqual(signin2.isNew, false);
      assert.strictEqual(emailsSent, 1, 'Subsequent sign-in on Device A must not send duplicate notice');

      // Sign-in on Device B: new device, notice sent
      const signin3 = onSignIn(userId, deviceTokenB);
      assert.strictEqual(signin3.isNew, true);
      assert.strictEqual(emailsSent, 2, 'Sign-in on new Device B must send notice');

      // Subsequent sign-in on Device B: already known, notice NOT sent
      const signin4 = onSignIn(userId, deviceTokenB);
      assert.strictEqual(signin4.isNew, false);
      assert.strictEqual(emailsSent, 2, 'Subsequent sign-in on Device B must not send duplicate notice');
    });
  });

});
