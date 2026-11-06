# Step 11: Per-User Customizable Dashboard Layouts (M18)

## Context & Ownership
- **Modules**: M18
- **Owner**: Dev
- **Size**: TBD
- **Status Note**: Sidebar navigation and narrow-screen responsive layouts were previously completed (PR #218). Per-user customizable dashboard widget layouts remain to be completed.

## Summary
Enable users to customize, persist, reorder, and hide/reveal dashboard widgets across devices, with workspace role permission enforcement ensuring viewers cannot alter default team layouts.

## Implementation & Verification Checklist
- [ ] Connect per-user dashboard layout state to `/api/v1/me/state/dashboard-layout` in `web/frontend/components/studio`.
- [ ] Support drag-and-drop / ordering controls for overview cards (KPIs, Risk Gauges, Recent Runs, Attribution).
- [ ] Add widget visibility toggles and "Reset to Default" action.
- [ ] Enforce read-only locks for viewers while allowing owners and editors to publish workspace defaults.
- [ ] Add layout migration and cross-device persistence automated tests.
