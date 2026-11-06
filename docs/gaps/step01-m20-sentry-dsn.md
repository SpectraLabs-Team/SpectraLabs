# Step 1: Set Production Sentry DSN and Confirm Test Event (M21, M20)

## Context & Ownership
- **Modules**: M21, M20
- **Owner**: Amogh R Gowda (decision confirmed Nov 5, 2026)
- **Size**: Small
- **Blocks**: Operational sign-off for M21 and M20

## Summary
Configure the production Sentry/GlitchTip DSN in the deployment environment and verify error ingestion with an intentional test event via the engine's observability probe.

## Implementation & Verification Checklist
- [ ] Set `SENTRY_DSN` in production vault / `deploy/production.env`.
- [ ] Confirm `spectra-api` and `spectra-worker` initialize the Sentry telemetry layer without panic.
- [ ] Dispatch an end-to-end verification error event using `POST /api/v1/observability/sentry/verify`.
- [ ] Verify receipt of the test event with correct environment, release tags, and request ID in the Sentry dashboard with owner Amogh R Gowda.
- [ ] Confirm alert rules trigger notifications to the production on-call channel.
