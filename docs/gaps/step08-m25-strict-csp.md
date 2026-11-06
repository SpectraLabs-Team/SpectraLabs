# Step 8: Strict CSP Scope Across All Routes (M25)

## Context & Ownership
- **Modules**: M25
- **Owner**: Dev
- **Size**: Small
- **Decision 6**: Strict CSP scope across all routes (approved Nov 5, 2026). OpenAPI annotations were previously merged in PR #217.

## Summary
Enforce a strict Content Security Policy (CSP) header across all application routes in Next.js middleware and Caddy edge reverse proxy, eliminating unsafe-inline/unsafe-eval where possible and locking down connect, frame, and script sources.

## Implementation & Verification Checklist
- [ ] Configure strict CSP header in `web/frontend/next.config.js` / middleware:
  - `default-src 'self'`.
  - `script-src 'self' 'nonce-...'` (or strict hash-based policies).
  - `style-src 'self' 'unsafe-inline'` (for Tailwind / emotion css).
  - `connect-src 'self' https://api.github.com https://*.sentry.io https://*.alpaca.markets`.
  - `frame-ancestors 'none'`.
- [ ] Mirror headers in `Caddyfile` for edge proxy enforcement.
- [ ] Validate zero CSP violation reports in browser console across Studio, Paper Lab, Learning Center, and Auth routes.
