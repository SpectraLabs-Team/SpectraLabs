import crypto from 'crypto';

export const BREACHED_PASSWORD_ERROR = 'This password has appeared in a data breach. Choose a different one.';

export interface BreachedCheckOptions {
  fetchImpl?: typeof fetch;
}

/**
 * Checks whether a password has appeared in public data breaches using the HaveIBeenPwned
 * range API (k-anonymity: only the first 5 chars of the SHA-1 hash are sent).
 *
 * - Header Add-Padding: true is sent to protect against response-length attacks.
 * - Padding rows have count 0; only count > 0 is considered breached.
 * - 3-second timeout.
 * - Fails OPEN (returns false + console.warn) on network error, timeout, or non-200.
 * - HIBP_DISABLED=1 skips the check.
 */
export async function isBreachedPassword(password: string, options: BreachedCheckOptions = {}): Promise<boolean> {
  if (process.env.HIBP_DISABLED === '1') return false;
  if (!password || typeof password !== 'string') return false;

  const fetchFn = options.fetchImpl || fetch;
  const sha1 = crypto.createHash('sha1').update(password, 'utf8').digest('hex').toUpperCase();
  const prefix = sha1.slice(0, 5);
  const suffix = sha1.slice(5);

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 3000);

  try {
    const res = await fetchFn(`https://api.pwnedpasswords.com/range/${prefix}`, {
      headers: {
        'Add-Padding': 'true',
        'User-Agent': 'SpectraLabs-Auth',
      },
      signal: controller.signal,
    });

    if (!res.ok) {
      console.warn(`[breached] HIBP range API returned HTTP ${res.status}; failing open.`);
      return false;
    }

    const text = await res.text();
    const lines = text.split(/\r?\n/);
    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      const [hashSuffix, countStr] = trimmed.split(':');
      if (hashSuffix && hashSuffix.toUpperCase() === suffix) {
        const count = parseInt(countStr, 10);
        return count > 0;
      }
    }
    return false;
  } catch (err: any) {
    console.warn(`[breached] HIBP check failed (${err?.message || err}); failing open.`);
    return false;
  } finally {
    clearTimeout(timer);
  }
}
