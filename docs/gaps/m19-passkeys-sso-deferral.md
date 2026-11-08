# M19: Passkeys & SSO Deferral Record

## Context & Ownership
- **Module**: M19 User & Security (95%)
- **Owner**: Owner Decision (Approved 8 Nov 2026)
- **Status**: Formally Recorded & Scheduled

## Deferral Record
1. **Scope Deferred**:
   - WebAuthn / FIDO2 Passkey authentication
   - Enterprise SSO (SAML 2.0 / Okta / Azure AD integration)
2. **Launch Readiness**:
   - Primary username/password authentication, TOTP-based two-factor authentication (2FA), and Google OAuth authentication are complete and verified on `main`.
   - Deferral does not block production launch or general availability.
3. **Target Date & Trigger**:
   - **Target Milestone**: Enterprise Workspace Tier
   - **Target Date**: Q2 2027
   - **Trigger**: Activation of enterprise workspace licensing and organization domain-level corporate IdP enforcement.
