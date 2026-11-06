# Step 9: Public API Keys and Developer Documentation (M20)

## Context & Ownership
- **Modules**: M20
- **Owner**: Dev
- **Size**: TBD

## Summary
Provide developer infrastructure for generating, rotating, and managing public API keys with scoped permissions, along with a comprehensive developer documentation portal.

## Implementation & Verification Checklist
- [ ] Database schema for public API keys (UUID, workspace reference, key prefix, SHA-256 hash, scopes, expiry, revoked_at).
- [ ] REST API endpoints for key management (`POST /api/v1/keys`, `GET /api/v1/keys`, `DELETE /api/v1/keys/{id}`).
- [ ] Bearer token authentication middleware authenticating external callers via public API keys.
- [ ] Interactive Developer Documentation portal mounted in frontend with OpenAPI Swagger / Redoc viewer.
- [ ] Code examples and quickstart guides (Python SDK, Node.js, and curl).
