// Static security headers (M19 task 14), used by next.config.js. Plain CommonJS so the config can
// require it and the tests can check it. The Content-Security-Policy itself is per request (it
// carries a nonce), so it lives in middleware.ts.

/**
 * Strict-Transport-Security: only once the app is served over HTTPS. HSTS=on|off forces it;
 * otherwise it is on when APP_URL is https://. includeSubDomains only with
 * HSTS_INCLUDE_SUBDOMAINS=1 (it would break any subdomain still on plain HTTP for a year).
 */
function hstsHeader(env = process.env) {
  const on = env.HSTS === 'on' || (env.HSTS !== 'off' && /^https:\/\//i.test(env.APP_URL || ''));
  if (!on) return null;
  return { key: 'Strict-Transport-Security', value: `max-age=31536000${env.HSTS_INCLUDE_SUBDOMAINS === '1' ? '; includeSubDomains' : ''}` };
}

function pageHeaders(env = process.env) {
  return [
    { key: 'X-Frame-Options', value: 'DENY' }, // no clickjacking (CSP frame-ancestors too)
    { key: 'X-Content-Type-Options', value: 'nosniff' },
    { key: 'Referrer-Policy', value: 'strict-origin-when-cross-origin' },
    { key: 'Permissions-Policy', value: 'camera=(), microphone=(), geolocation=(), payment=(), usb=(), interest-cohort=()' },
    // Other sites can't keep a handle on our window (window.opener attacks, some side channels).
    { key: 'Cross-Origin-Opener-Policy', value: 'same-origin' },
    { key: 'X-Permitted-Cross-Domain-Policies', value: 'none' },
    ...[hstsHeader(env)].filter(Boolean),
  ];
}

/** API responses are JSON: nothing in them should ever run or be framed. */
const apiHeaders = [
  { key: 'Cache-Control', value: 'no-store' }, // per-user data; never in a shared cache
  { key: 'Content-Security-Policy', value: "default-src 'none'; frame-ancestors 'none'" },
];

module.exports = { hstsHeader, pageHeaders, apiHeaders };
