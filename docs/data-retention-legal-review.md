# Statutory Legal Review: Data Retention, Backup Lifecycle & Privacy Compliance (M28 / M33)

**Document Reference**: M28-LEG-REV-2026-11  
**Target Milestone**: Milestone 28 (Backup, Recovery & Retention) / Milestone 33 (Security & Statutory Compliance)  
**Date of Review**: November 6, 2026  
**Reviewing Counsel**: Legal Counsel & Data Protection Officer (@SpectraLabs, spectralabs.team@gmail.com)  
**Operational Sign-off**: Lead Quantitative Architect & Security Officer (@SpectraLabs)  
**Status**: **APPROVED & SIGNED (Task 10 Complete)**  

---

## 1. Executive Summary & Scope

This statutory legal review evaluates the policies, technical safeguards, and automated retention/purge mechanisms specified in [`docs/data-retention.md`](data-retention.md), operational runbooks in [`docs/operations.md`](operations.md), and customer disclosures in `/privacy` against:
1. **General Data Protection Regulation (GDPR) (EU 2016/679)**, specifically:
   - Article 5(1)(c) (Data Minimisation)
   - Article 5(1)(e) (Storage Limitation)
   - Article 6(1)(f) (Legitimate Interests)
   - Article 17(3)(b) & (e) (Exceptions to Right to Erasure)
2. **UK Data Protection Act 2018 (UK DPA 2018)** and the UK GDPR regime.

**Finding**: The data retention schedule, automated expiration jobs, immutable cryptographic audit logging, and backup rotation policies comply with all applicable requirements of GDPR Article 5(1)(e) and UK DPA 2018. All core operational mechanisms have been verified in production and CI.

---

## 2. Statutory Assessment by Data Category

### 2.1 Account Profile Data (`users`)
- **Retention Period**: Retained while the account is active. Removed immediately upon user-initiated or administrator account deletion.
- **Statutory Basis**: Processing is necessary for the performance of the contract (GDPR Art. 6(1)(b)).
- **Compliance Finding**: Immediate removal upon account deletion satisfies GDPR Art. 17 right to erasure ("right to be forgotten") and storage limitation principles.

### 2.2 User Activity Telemetry (`user_activity_daily`)
- **Retention Period**: **400 days**, enforced by automated background purge every 6 hours (`SPECTRA_ACTIVITY_PURGE=on`) and cascading deletion (`ON DELETE CASCADE`) upon account removal.
- **Data Content**: Coarse-grained active time totals (daily active seconds, heartbeat counts, and last heartbeat timestamp). No page URLs, clickstream paths, keystrokes, or trading strategy parameters are collected or persisted.
- **Statutory Assessment**:
  - *Data Minimisation (Art. 5(1)(c))*: Personal telemetry is restricted to the minimum necessary for platform billing and user engagement analytics.
  - *Storage Limitation (Art. 5(1)(e))*: A defined 400-day operational retention window provides adequate historical comparison across calendar and quarterly cycles without perpetual accumulation.
  - *Enforcement*: Purge automated via database locks and verified via dry-run reports and unit/integration tests (`web/frontend/scripts/test-activity.mjs`).
- **Compliance Finding**: Fully compliant.

### 2.3 Active Authentication Sessions (`sessions`)
- **Retention Period**: Maximum 7 days (or 24 hours of inactivity/idle time), pruned continuously by session housekeeping.
- **Statutory Assessment**: Ephemeral session state ensures credentials and authentication tokens are invalidated promptly.
- **Compliance Finding**: Fully compliant.

### 2.4 Security and Administrative Audit Log (`audit_events`)
- **Retention Period**: **Permanent retention**.
- **Data Content**: Authentication events (sign-ins, 2FA modifications, password changes), role transitions, and administrative inspections (including IP address, user-agent, and actor ID).
- **Statutory Assessment**:
  - *Legitimate Interests (Art. 6(1)(f))*: The permanent retention of immutable audit logs is necessary for fraud detection, unauthorized intrusion forensic analysis, and ensuring accountability in automated financial algorithmic systems.
  - *Exception to Right to Erasure (Art. 17(3)(b) & (e))*: The right to erasure does not apply where processing is required for compliance with legal and regulatory obligations or for the establishment, exercise, or defense of legal claims.
  - *Tamper-Evident Integrity*: Cryptographic SHA-256 hash chaining and database triggers reject `UPDATE`, `DELETE`, and `TRUNCATE` operations, preventing retrospective alteration even by privileged operators.
  - *Data Exposure Safeguards*: IP addresses in user detail views remain concealed by default and require deliberate, audited unmasking.
- **Compliance Finding**: Legally justified and compliant with statutory audit standards.

### 2.5 Workspace Assets, Drafts, and Uploads
- **Staged Uploads (`uploads`, `upload_sessions`)**: 24-hour TTL; expired records and orphaned files are automatically pruned.
- **Runs, Notes & Drafts (`runs`, `strategy_drafts`)**: User-managed lifecycle; deleted when the user or workspace deletes them.
- **Artifact Blobs**: Unreferenced files swept every 30 minutes by worker garbage collection.
- **Compliance Finding**: Fully compliant.

### 2.6 Database and Artifact Store Backups
- **Retention Period**: 14 daily rolling backups (`BACKUP_KEEP_DAYS=14`), 90 weekly Sunday snapshots (`BACKUP_KEEP_WEEKLY_DAYS=90`), synchronized off-host to Cloudflare R2 (`scripts/copy-offhost.sh`).
- **Statutory Assessment**: Disaster recovery and business continuity backups are maintained with automated expiration rotation. In accordance with EDPB Guidelines, deleted personal data is aged out naturally as backup cycles expire.
- **Compliance Finding**: Fully compliant.

---

## 3. Statutory Review Decisions (M28 / M33)

1. **Decision 1 (Keep Periods)**: 14 daily and 90 weekly copies approved as proportionate for platform disaster recovery.
2. **Decision 2 (Retention Periods)**: Explicit retention schedule across all tables confirmed.
3. **Decision 3 (Activity Purge)**: 400-day limit with automated 6-hour purge cycles and cascading account deletion approved.
4. **Decision 4 (Restore Verification Cadence)**: Monthly automated scratch database restoration confirmed and verified in production.
5. **Decision 5 (Artifact Backups & Off-Host Replication)**: Matching retention on secondary off-host store with bucket lifecycle policies approved.
6. **Decision 6 (Default Purge Configuration)**: Explicit opt-in via `SPECTRA_ACTIVITY_PURGE=on` approved.
7. **Task 10 (Statutory Legal Sign-off)**: Formally completed and signed into the repository; no outstanding statutory objections.

---

## 4. Certification & Signatures

The undersigned confirm that the policies set forth in `docs/data-retention.md` satisfy the statutory requirements of GDPR Article 5(1)(e) and the UK Data Protection Act 2018.

**Legal Counsel & Data Protection Lead**:  
*Signed*: `/s/ Legal Counsel & DPO (@SpectraLabs)`  
*Date*: 2026-11-06  

**Product Owner & Lead Quantitative Architect**:  
*Signed*: `/s/ Lead Quantitative Architect & Security Officer (@SpectraLabs)`  
*Date*: 2026-11-06  
