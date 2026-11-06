# Step 7: Execution-Risk Controls, Alerts (M8b), Decision Replay, Exposure View (M8)

## Context & Ownership
- **Modules**: M8
- **Owner**: Dev
- **Size**: Large
- **Status Note**: PR #220 updated Risk Center frontend positions only. Server-side controls, alerts (M8b), decision replay, and exposure views are tracked here for full gap closure.

## Summary
Provide server-side execution risk controls validation against live positions and open orders, alert threshold monitoring, deterministic decision replay verification, and exposure time-series aggregation.

## Implementation & Verification Checklist
- [ ] Server-side execution risk controls validation against live positions and open orders with fail-closed rejection semantics.
- [ ] Alert threshold monitoring (>80% utilization warnings and hard limit breach notifications).
- [ ] Deterministic decision replay verification endpoint (`POST /api/v1/risk/replay/{decision_id}`) and Studio verification badge.
- [ ] Exposure history time-series endpoint (`GET /api/v1/risk/exposure-history`) and Studio exposure chart.
- [ ] End-to-end integration tests verifying pre-trade rejection and replay reproduction.
