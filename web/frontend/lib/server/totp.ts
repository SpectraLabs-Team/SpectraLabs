import crypto from 'crypto';

/**
 * Time-based one-time passwords (RFC 6238 / RFC 4226): HMAC-SHA1, 30-second steps, 6 digits,
 * the defaults every authenticator app (Google Authenticator, 1Password, Authy…) uses.
 * Pure functions; storage and replay protection live in ./mfa.
 */
export const TOTP_PERIOD_S = 30;
export const TOTP_DIGITS = 6;
/** Accept the previous and next step too (clock drift between phone and server). */
export const TOTP_WINDOW = 1;
export const ISSUER = 'Spectra Labs';

const B32 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';

export function base32Encode(buf: Buffer): string {
  let bits = 0;
  let value = 0;
  let out = '';
  for (const byte of buf) {
    value = (value << 8) | byte;
    bits += 8;
    while (bits >= 5) {
      out += B32[(value >>> (bits - 5)) & 31];
      bits -= 5;
    }
  }
  if (bits > 0) out += B32[(value << (5 - bits)) & 31];
  return out;
}

export function base32Decode(s: string): Buffer {
  const clean = s.toUpperCase().replace(/[\s=-]/g, '');
  const out: number[] = [];
  let bits = 0;
  let value = 0;
  for (const ch of clean) {
    const v = B32.indexOf(ch);
    if (v < 0) throw new Error('invalid base32');
    value = (value << 5) | v;
    bits += 5;
    if (bits >= 8) {
      out.push((value >>> (bits - 8)) & 255);
      bits -= 8;
    }
  }
  return Buffer.from(out);
}

/** A new random 160-bit secret (the RFC 4226 recommended length). */
export const generateTotpSecret = () => crypto.randomBytes(20);

export const stepAt = (ms: number) => Math.floor(ms / 1000 / TOTP_PERIOD_S);

/** The code for one time step (HOTP with counter = step). */
export function totpAt(secret: Buffer, step: number, digits = TOTP_DIGITS): string {
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(step));
  const mac = crypto.createHmac('sha1', secret).update(counter).digest();
  const offset = mac[mac.length - 1] & 0x0f;
  const bin = ((mac[offset] & 0x7f) << 24) | (mac[offset + 1] << 16) | (mac[offset + 2] << 8) | mac[offset + 3];
  return String(bin % 10 ** digits).padStart(digits, '0');
}

/**
 * The time step a code matches (within ±TOTP_WINDOW of now), or null. Compares in constant time.
 * Callers must reject a step at or before the last one used (replay protection).
 */
export function matchTotp(secret: Buffer, code: string, now = Date.now()): number | null {
  const clean = String(code).replace(/\s/g, '');
  if (!/^\d{6}$/.test(clean)) return null;
  const want = Buffer.from(clean);
  const current = stepAt(now);
  let found: number | null = null;
  for (let d = -TOTP_WINDOW; d <= TOTP_WINDOW; d++) {
    const got = Buffer.from(totpAt(secret, current + d));
    if (crypto.timingSafeEqual(got, want) && found === null) found = current + d;
  }
  return found;
}

/** otpauth:// URI for the QR code (Key URI Format used by authenticator apps). */
export function otpauthUri(accountEmail: string, secret: Buffer): string {
  const label = encodeURIComponent(`${ISSUER}:${accountEmail}`);
  const params = new URLSearchParams({
    secret: base32Encode(secret),
    issuer: ISSUER,
    algorithm: 'SHA1',
    digits: String(TOTP_DIGITS),
    period: String(TOTP_PERIOD_S),
  });
  return `otpauth://totp/${label}?${params.toString().replace(/\+/g, '%20')}`;
}
