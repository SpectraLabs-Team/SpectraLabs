# M25 Task 1: browser-stored state inventory

Checked against the working tree on branch `m28/nightly-backup-install` (base origin/main 730f992).
Classification follows the plan's Decisions 1 to 8.

## Per-user data: migrate to server (Decision 1, Decision 8)

| Key | Where | Contents | Action |
|---|---|---|---|
| `spectra-portfolio-<email>` | PortfolioContext.tsx | portfolio positions as CSV plus source | Migrate, per user |
| `spectra-paper-<email>` | PortfolioContext.tsx | paper-account state | Migrate, per user. Reconcile with paper_orders (see Open issue 1) |
| `spectra-risk-params-<email>` | PortfolioContext.tsx | risk parameters | Migrate, per user |
| `spectra-paper-sandbox-v1` | PaperLab.tsx | sandbox cash, positions, orders | Migrate, per user. Holds an orders array (see Open issue 1) |
| `spectra-paper-watchlist` | PaperLab.tsx | watchlist symbols | Migrate, per user |

## Dropped (Decision 7)

| Key | Where | Action |
|---|---|---|
| `spectra-alpaca-account-cache` | PaperLab.tsx | Drop. Refetch from Alpaca on load |

## Browser-only (Decision 4)

| Key | Where | Notes |
|---|---|---|
| `spectra-sidebar-collapsed` | StudioShell.tsx | Stays local |
| `paper.watchlistOpen` | PaperLab.tsx | Stays local |

## Never stored (Decision 6)

| Item | Where | Notes |
|---|---|---|
| Alpaca secret-key input | PaperLab.tsx `inputSecretKey` | React state only. Not written to any storage key found in this inventory. Keep it out of the import and out of server logs |

## Transient (found in review, not persisted)

| Key | Where | Class |
|---|---|---|
| `spectra-upload:<name>:<size>:<lastModified>` | web/frontend/lib/datasets.ts | Browser-only, resumable upload session IDs |
| `spectra-auth-next` (sessionStorage) | AuthModal.tsx, NavLink.tsx, InviteAccept.tsx | Browser-only, post-login redirect path |
| `spectra_landing_pending_anchor` (sessionStorage) | NavLink.tsx | Browser-only, landing scroll target |

## Not in the plan's inventory (found in this pass)

These are browser-stored and were not listed in the plan. None is Studio per-user portfolio data, so they are not migration candidates unless the owner says otherwise. Confirm each.

| Key | Where | Suggested class |
|---|---|---|
| `spectra-workspace-guide-open` | WorkspaceGuide.tsx | Browser-only (view setting) |
| `spectra-theme` | lib/theme.tsx | Browser-only (view setting) |
| `spectra-learn-nav-collapsed` | learn/LearnChrome.tsx | Browser-only (view setting) |
| `spectra-auth` (sessionStorage) | lib/auth.tsx | Session cache, not persisted |
| `spectra-auth-sync` | lib/auth.tsx | Cross-tab sync signal, not persisted |
| `onboardingStorageKey(email)` | onboarding/useOnboardingState.ts | Per-user. Has a server copy, so a local cache. Confirm |
| `getLayoutStorageKey(workspaceId)` | lib/dashboardLayout.ts | Per-workspace dashboard layout. Migrate or keep as view setting. Owner decision |

## Open issues

1. **Second order book.** `spectra-paper-sandbox-v1` stores an `orders` array, and `spectra-paper-<email>` may also hold paper orders. Decision 8 says `paper_orders` is the only order book. Owner needs to decide: migrate sandbox orders into a sandbox-scoped table, or drop them. Do not migrate them into `paper_orders` without a decision.
2. **Dashboard layout and onboarding** were not in the plan. Owner decision on each (see table above).
3. Account deletion. `AccountPage.tsx` lines 342 to 343 remove `spectra-portfolio-<email>` and `spectra-paper-<email>` but not `spectra-risk-params-<email>`. Add that removal. Server-side state needs no extra step: every `user_*` table has `user_id ... ON DELETE CASCADE`.

## Method

Searched `web/frontend/components`, `lib` and `app` for `localStorage` and `sessionStorage` get, set and remove calls, then resolved each constant or key function. The `app` directory had no storage calls. Not searched: other directories under `web/` outside `web/frontend`.
