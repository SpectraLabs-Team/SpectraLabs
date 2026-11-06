# Step 5: Google Sign-in Integration (M19)

## Context & Ownership
- **Modules**: M19
- **Owner**: Dev
- **Size**: Medium
- **Status Note**: PR #220 relaxed email delivery startup requirements only. Google OAuth sign-in remains an open gap. Passkeys and SAML/SSO are deferred to subsequent phases.

## Summary
Provide Google OAuth 2.0 authentication alongside email/password and magic-link sign-in, seamlessly linking Google profiles to PostgreSQL user accounts.

## Implementation & Verification Checklist
- [ ] Add `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` configuration to environment specifications (`.env.example`, `web/frontend/.env.example`).
- [ ] Configure Google OAuth provider in `web/frontend/lib/server/authStore.ts` / NextAuth handlers.
- [ ] Support account linking for existing email accounts and provision new users with default workspaces on first sign-in.
- [ ] Add Google Sign-in button to authentication dialogs (`AuthModal.tsx`).
- [ ] Add automated auth flow tests covering OAuth code exchange and session issuance.
