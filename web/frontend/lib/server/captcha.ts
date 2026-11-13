import crypto from 'crypto';

/**
 * Bot protection and CAPTCHA management (M30 Tasks 3 & 4).
 *
 * Provider: Cloudflare Turnstile (privacy-respecting, zero cross-site trackers).
 * Actions: 'signup' | 'login' | 'reset' - strictly bound to the token to prevent cross-action replay.
 * Outage Policy (Decision 3):
 *   - Sign-up & Reset: Fail closed (prevent bot abuse).
 *   - Sign-in: Fail open with rate limit still in force (prevent user lockouts).
 * Security: Tokens are single-use, expire after 5 minutes, and test-mode is strictly prohibited in production.
 */

export type CaptchaAction = 'signup' | 'login' | 'reset';

export type CaptchaVerificationResult = {
  ok: boolean;
  reason?:
    | 'missing'
    | 'expired'
    | 'invalid'
    | 'replay'
    | 'action_mismatch'
    | 'outage_fail_closed'
    | 'outage_fail_open'
    | 'test_mode_rejected_in_prod';
};

// ── In-Memory Cache for Single-Use & Failure Bounding ──

const TOKEN_TTL_MS = 5 * 60 * 1000; // 5 minutes
const usedTokens = new Map<string, number>();

function pruneUsedTokens(now = Date.now()): void {
  for (const [hash, exp] of usedTokens.entries()) {
    if (exp <= now) usedTokens.delete(hash);
  }
}

/** Test dummy secrets allowlisted for development / testing */
export const TURNSTILE_TEST_PASS_TOKEN = '1x0000000000000000000000000000000AA';
export const TURNSTILE_TEST_FAIL_TOKEN = '2x0000000000000000000000000000000AA';

/** Verify a CAPTCHA token with single-use, expiry, action binding, and test-mode gating. */
export async function verifyCaptchaToken(
  token: string | undefined | null,
  action: CaptchaAction,
  options: {
    ip?: string | null;
    mockProviderStatus?: 'ok' | 'outage';
    issuedAt?: number;
    tokenAction?: CaptchaAction;
  } = {},
): Promise<CaptchaVerificationResult> {
  if (!token || typeof token !== 'string' || !token.trim()) {
    return { ok: false, reason: 'missing' };
  }

  const cleanToken = token.trim();
  const isProd = process.env.NODE_ENV === 'production';
  const now = Date.now();

  // Test mode token check
  const isTestToken =
    cleanToken === TURNSTILE_TEST_PASS_TOKEN ||
    cleanToken === TURNSTILE_TEST_FAIL_TOKEN ||
    cleanToken.startsWith('test_token_');

  if (isTestToken) {
    if (isProd) {
      // Test mode is strictly prohibited in production (Task 3)
      return { ok: false, reason: 'test_mode_rejected_in_prod' };
    }
    if (cleanToken === TURNSTILE_TEST_FAIL_TOKEN) {
      return { ok: false, reason: 'invalid' };
    }
  }

  // Token Expiration check
  if (options.issuedAt && now - options.issuedAt > TOKEN_TTL_MS) {
    return { ok: false, reason: 'expired' };
  }

  // Action binding check (Task 3: sign-up token cannot be used on sign-in or reset)
  if (options.tokenAction && options.tokenAction !== action) {
    return { ok: false, reason: 'action_mismatch' };
  }

  // Parse simulated embedded metadata (e.g. "token:<action>:<timestamp>:<nonce>")
  if (cleanToken.startsWith('tk:')) {
    const parts = cleanToken.split(':');
    if (parts.length >= 3) {
      const boundAction = parts[1] as CaptchaAction;
      const timestamp = parseInt(parts[2], 10);
      if (boundAction && boundAction !== action) {
        return { ok: false, reason: 'action_mismatch' };
      }
      if (!isNaN(timestamp) && now - timestamp > TOKEN_TTL_MS) {
        return { ok: false, reason: 'expired' };
      }
    }
  }

  // Single-use / Replay guard
  pruneUsedTokens(now);
  const tokenHash = crypto.createHash('sha256').update(cleanToken).digest('hex');
  if (usedTokens.has(tokenHash)) {
    return { ok: false, reason: 'replay' };
  }
  usedTokens.set(tokenHash, now + TOKEN_TTL_MS);

  // Provider outage handling (Decision 3)
  if (options.mockProviderStatus === 'outage') {
    if (action === 'login') {
      // Fail open on sign-in with rate limit still in force
      return { ok: true, reason: 'outage_fail_open' };
    }
    // Fail closed on sign-up and reset
    return { ok: false, reason: 'outage_fail_closed' };
  }

  // Provider remote verification (Cloudflare Turnstile siteverify)
  const secretKey = process.env.TURNSTILE_SECRET_KEY;
  if (!secretKey && !isTestToken) {
    // If provider is not configured: in production fail closed, in dev permit pass
    if (isProd) {
      return action === 'login' ? { ok: true, reason: 'outage_fail_open' } : { ok: false, reason: 'outage_fail_closed' };
    }
    return { ok: true };
  }

  if (secretKey && !isTestToken) {
    try {
      const formData = new URLSearchParams();
      formData.append('secret', secretKey);
      formData.append('response', cleanToken);
      if (options.ip) formData.append('remoteip', options.ip);

      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 4000);

      const res = await fetch('https://challenges.cloudflare.com/turnstile/v0/siteverify', {
        method: 'POST',
        body: formData,
        signal: controller.signal,
      });
      clearTimeout(timeout);

      const data = (await res.json()) as { success: boolean; action?: string; 'error-codes'?: string[] };
      if (!data.success) {
        return { ok: false, reason: 'invalid' };
      }
      if (data.action && data.action !== action) {
        return { ok: false, reason: 'action_mismatch' };
      }
      return { ok: true };
    } catch {
      // Network outage / timeout contacting provider
      if (action === 'login') {
        return { ok: true, reason: 'outage_fail_open' };
      }
      return { ok: false, reason: 'outage_fail_closed' };
    }
  }

  return { ok: true };
}

// ── Adaptive Login Failures per Account and IP (Task 4) ──

const FAIL_WINDOW_MS = 15 * 60 * 1000; // 15-minute fixed window
const FAIL_LIMIT_ACCOUNT = 3; // require CAPTCHA after 3 failures for an account
const FAIL_LIMIT_IP = 5; // require CAPTCHA after 5 failures from an IP

type FailureRecord = { count: number; expiresAt: number };
const accountFailures = new Map<string, FailureRecord>();
const ipFailures = new Map<string, FailureRecord>();

function getFailureCount(map: Map<string, FailureRecord>, key: string, now = Date.now()): number {
  const rec = map.get(key);
  if (!rec) return 0;
  if (rec.expiresAt <= now) {
    map.delete(key);
    return 0;
  }
  return rec.count;
}

export function recordLoginFailure(email: string, ip: string, now = Date.now()): { accountCount: number; ipCount: number } {
  const cleanEmail = email.trim().toLowerCase();

  // Update account failures
  const accRec = accountFailures.get(cleanEmail);
  const accExpiresAt = accRec && accRec.expiresAt > now ? accRec.expiresAt : now + FAIL_WINDOW_MS;
  const accCount = (accRec && accRec.expiresAt > now ? accRec.count : 0) + 1;
  accountFailures.set(cleanEmail, { count: accCount, expiresAt: accExpiresAt });

  // Update IP failures
  const ipRec = ipFailures.get(ip);
  const ipExpiresAt = ipRec && ipRec.expiresAt > now ? ipRec.expiresAt : now + FAIL_WINDOW_MS;
  const ipCount = (ipRec && ipRec.expiresAt > now ? ipRec.count : 0) + 1;
  ipFailures.set(ip, { count: ipCount, expiresAt: ipExpiresAt });

  return { accountCount: accCount, ipCount };
}

export function isCaptchaRequiredOnLogin(email: string, ip: string, now = Date.now()): boolean {
  const cleanEmail = email.trim().toLowerCase();
  const accCount = getFailureCount(accountFailures, cleanEmail, now);
  const ipCount = getFailureCount(ipFailures, ip, now);
  return accCount >= FAIL_LIMIT_ACCOUNT || ipCount >= FAIL_LIMIT_IP;
}

export function clearLoginFailures(email: string, ip: string): void {
  accountFailures.delete(email.trim().toLowerCase());
  ipFailures.delete(ip);
}

// ── Generic Security Error Message (Prevents Email Enumeration) ──

export const GENERIC_AUTH_ERROR = 'Incorrect email or password. Please try again.';
export const CAPTCHA_REQUIRED_ERROR = 'Incorrect email or password. Security verification required to continue.';
