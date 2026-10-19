# Security

## Reporting a problem

Email **spectralabs.team@gmail.com** with "Security" in the subject. Please don't open a public
issue for anything that could be exploited.

## What runs automatically

| Where | What | Fails when |
| --- | --- | --- |
| `.github/workflows/security.yml` (every push, pull request, and weekly) | **gitleaks** over the whole git history | A key, token or password is committed anywhere, even if a later commit removed it |
| | **npm audit** on `web/frontend/package-lock.json` | A production dependency has a high or critical advisory, or any dependency has a critical one |
| | **cargo audit** on `Cargo.lock` | Any RustSec advisory, including unmaintained, yanked or unsound crates |
| `.github/workflows/ci.yml` → `web` | Typecheck and `npm test` against Postgres | Any auth or security test fails, including the permission rule for every API route |
| `.github/dependabot.yml` | Weekly update pull requests (npm, cargo, GitHub Actions) | (Security fixes are opened as soon as an advisory is published.) |

**If gitleaks finds a real secret:** rotate it first (deleting the commit doesn't un-leak it), then
remove it. Fake values used in tests go in `.gitleaks.toml` as exact strings, never as whole files.

**Permission rules:** `scripts/test-auth.mjs` lists every API route with the check it must make
first (public, signed in, admin, or a workspace role). A new route fails the tests until it is
listed there, so nothing ships without a deliberate decision about who can call it.

## Alerts

The app counts failed sign-ins and emailed-code requests site-wide. When either spikes, the
team gets one alert per hour per kind: a `security.alert` audit event, a `[SECURITY ALERT]` server
log line, an email, and an optional webhook post. Configure in `web/frontend/.env.local`:

| Variable | Default | |
| --- | --- | --- |
| `ALERT_FAILED_LOGINS` | `50/10` | count/minutes, or `off` |
| `ALERT_CODE_REQUESTS` | `100/10` | count/minutes, or `off` |
| `ALERT_EMAIL` | every active platform admin | comma-separated |
| `ALERT_WEBHOOK_URL` | (none) | Slack- or Discord-compatible incoming webhook |

**When an alert arrives:** open the audit log in the admin area and filter on `auth.login.failure`.
- Many accounts from a few addresses: credential stuffing. Block those addresses at the proxy or
  host firewall. Accounts are already locked after 10 wrong passwords.
- One account from many addresses: a targeted guess. Contact the owner, suggest an authenticator app.
- Code requests for addresses that never asked: someone is using our emails to spam. Tighten
  `otp:ip` limits in `app/api/auth/send-otp/route.ts` or block the source.

Raise thresholds once real traffic makes the defaults noisy; don't turn alerts off.

## Before real-money trading

Real-money trading stays off until all of these are done:

- [ ] An **external penetration test** by an independent firm, covering: sign-in, two-step
  login, recovery and password reset; session handling; workspace isolation (one customer reading or
  changing another's data, runs or broker keys); the `/api/engine` proxy and the engine's identity
  check; broker-key storage; the admin area; CSP and cross-site request protection.
- [ ] Every high or critical finding fixed and re-tested; medium findings fixed or accepted in
  writing.
- [ ] `CSP_MODE=enforce` in production with no unexplained violation reports for two weeks.
- [ ] The security workflow green on `main`, with branch protection requiring it.
- [ ] `SECRETS_MASTER_KEY` and `AUTH_SECRET` held only in the production secret store, with a
  tested restore of the master key from its offline backup.
- [ ] `npm run audit:verify` run on a schedule, with failures alerting the team.
