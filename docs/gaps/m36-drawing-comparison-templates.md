# M36 Drawing, Comparison & Templates: Module Implementation Tracker

## Context & Scope
- **Module**: M36 Drawing Tools, Symbol Comparison & Saved Layout Templates
- **State**: Complete (100%)
- **Specification**: `docs/models/m36-drawing-templates-spec.md`
- **Dependencies**: M35 Charting & Indicators (Done, 100%), M18 Dashboard (Done)

## Task Implementation Tracker

| Task | Scope & Deliverable | Branch / PR | Status |
| :--- | :--- | :--- | :---: |
| **Task 0: Design & Decisions** | Storage in DB per user/workspace, rebase to 100 on first dataset bar, soft delete, workspace template limits, and performance budget in `docs/models/m36-drawing-templates-spec.md`. | `m36-drawing-templates` | Complete |
| **Task 1: Drawing Model & API** | DB migrations (`chart_drawings`), DB queries, REST API (`/api/v1/charts/{chart_id}/drawings`, `/api/v1/drawings/{id}`), viewer 403 enforcement, 200-drawing count limit, 64KB payload limit, and soft-delete tests. | `m36-drawing-templates` | Complete |
| **Task 2: Drawing Tools on Canvas** | Interactive Trendline, Horizontal Level, Fibonacci Retracement, Text annotation, mobile touch support (drag move, tap select), Undo/Redo, and <16ms render budget verification. | `m36-drawing-templates` | Complete |
| **Task 3: Symbol Comparison API** | `GET /api/v1/charts/compare` rebased to 100 on first bar of full dataset, timestamp calendar alignment with null gaps, native interval matching (422), workspace dataset membership validation, and sealed-holdout check. | `m36-drawing-templates` | Complete |
| **Task 4: Saved Layout Templates** | DB table (`chart_templates`), REST API (`/api/v1/chart-templates`), 50-template limit, retired dataset version mismatch detection, and sealed holdout prevention. | `m36-drawing-templates` | Complete |
| **Task 5: UI Integration & Verification** | Studio chart integration (toolbar, comparison overlay modal, template save/load modal), test suites at desktop (1280px) and mobile (375px), full CI verification. | `m36-drawing-templates` | Complete |

## Verification Summary
- **Backend Tests**:
  - `chart_drawings_store`: 3/3 passed (`drawing_limits_and_validation`, `drawing_lifecycle_create_read_update_soft_delete`, `chart_templates_lifecycle_and_limits`).
  - `chart_drawings_flow`: 3/3 passed (`test_symbol_comparison_flow`, `test_chart_templates_flow`, `test_drawings_crud_and_limits`).
  - `openapi::tests::the_spec_lists_every_documented_route`: 1/1 passed.
- **Frontend Tests**:
  - `test-chart-drawings.mjs`: 8/8 passed (models, toolbar, touch, shortcuts, <16ms 200-drawing budget).
  - `test-symbol-comparison.mjs`: 10/10 passed (API client, formula 100*(Pt/P0), null gaps, modal, legend).
  - `test-chart-templates.mjs`: 8/8 passed (CRUD client, 50-limit, retired dataset alerts, viewer read-only rule).
  - Full suite (`npm test`): 27/27 test suites passed.
- **Linters**:
  - `cargo fmt --check`: passed (0 diffs).
  - `cargo clippy --all-targets -- -D warnings`: passed (0 warnings).
  - `npm run typecheck`: passed (0 errors).
  - `npm run lint`: passed (0 errors).
