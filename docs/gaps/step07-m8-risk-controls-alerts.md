# Step 7: Execution-Risk Controls, Alerts (M8b), Decision Replay, Exposure View (M8)

## Context & Ownership
- **Modules**: M8
- **Owner**: Dev
- **Size**: Large
- **Status Note**: PR #220 updated Risk Center frontend positions only. Server-side controls, alerts (M8b), decision replay, and exposure views are tracked here for full gap closure.

## Summary
Provide server-side execution risk controls validation against live positions and open orders, alert threshold monitoring, deterministic decision replay verification, and exposure time-series aggregation.

## Implementation & Verification Checklist
- [x] Server-side execution risk controls validation against live positions and open orders with fail-closed rejection semantics.
- [x] Alert threshold monitoring (>80% utilization warnings and hard limit breach notifications across gross, net, leverage, drawdown, daily loss, position size, concentration, and VaR).
- [x] Deterministic decision replay verification endpoint (`POST /api/v1/risk/replay/{decision_id}`) and Studio verification badge.
- [x] Exposure history time-series endpoint (`GET /api/v1/risk/exposure-history`) and Studio exposure chart.
- [x] End-to-end integration tests verifying pre-trade rejection and replay reproduction.
- [x] Risk alert delivery and M27 email channel staging endpoint (`POST /api/v1/risk/alerts/dispatch`) with Studio trigger.
