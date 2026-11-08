# M36: Drawing Tools, Symbol Comparison & Saved Layout Templates Specification

**Status**: Signed off per M36 task plan recommendations  
**Reference Document**: `M36 Drawing, Comparison & Templates: task plan`  
**Target Completion**: 100% complete across Phase M36a and Phase M36b  

---

## 1. Architectural Decisions & Scope

### Decision 1: Drawing & Layout Storage
- **Choice**: Stored server-side in PostgreSQL per user and workspace.
- **Rationale**: Drawings and chart templates must survive browser reloads, page navigation, device switching, and sessions across multiple machines.
- **Permissions**:
  - **Viewers**: Can read drawings and workspace templates (`role = 'viewer'`), but cannot save, update, or delete them (returns `403 Forbidden`).
  - **Editors / Admins**: Full write permissions to create, update, and soft-delete drawings and templates.

### Decision 2: Comparison Scaling & Base Bar
- **Choice**: Rebase symbols to 100.0 at the **first bar of the full dataset**.
- **Formula**: $\text{Rebased}_t = 100.0 \times \frac{P_t}{P_{\text{base}}}$ where $P_{\text{base}}$ is the close price of the first bar in the dataset.
- **Rationale**: Setting the base to the first bar of the full dataset ensures that the relative percentage curves remain completely stable during user panning and scrolling, rather than jumping as new bars enter the visible canvas window.
- **Missing Bars & Calendar Alignment**:
  - The series are aligned by UTC bar timestamp.
  - If a symbol lacks a bar at timestamp $t$ (e.g., market holiday, half day, differing exchange hours), the output for that timestamp is `null` (gap). No fictitious prices are interpolated.
- **Timeframe & Interval**:
  - Timeframe must strictly match the dataset's native bar interval (e.g. `1d`, `1h`, `1m`). Timeframe mismatches return `422 Unprocessable Entity` rather than silently resampling.
- **Dataset Scope**:
  - The secondary comparison symbol must exist within the active workspace's datasets. If it belongs to another workspace or is not loaded, a `404 Not Found` ("symbol not in workspace datasets") is returned.
- **Sealed-Holdout Safety**:
  - The comparison endpoint reuses the sealed-holdout check from M35 (`datasets_api::load_for_chart`) to prevent leaking test/holdout intervals.

### Decision 3: Soft Delete & Undo/Redo
- **Soft Delete**: Chart drawings have a `deleted_at` timestamp. Deletion sets `deleted_at = now()`. Soft-deleted drawings are excluded from default queries but can be recovered or audited.
- **Undo/Redo**: Client-side in-memory undo/redo stack during active editing sessions; database soft delete ensures accidental deletions can be restored.

### Decision 4: Template Versioning & Boundaries
- **Version Number**: Templates carry an explicit schema version integer (`version = 1`).
- **Retired Dataset Check**: Each element records the `dataset_version_id` it was authored against. If that dataset version is retired or missing, the template explicitly reports the missing version rather than silently drifting to another dataset.
- **Workspace Limits**:
  - Max **200 drawings** per chart.
  - Max **50 saved templates** per workspace.
  - Max **64 KB** payload per individual drawing.

### Decision 5: Performance Budget
- **Canvas Rendering Budget**: A chart with 200 active drawings must render and respond to panning/zooming within **16 ms** (60 fps interactive frame budget; hard ceiling 50 ms).

---

## 2. Drawing Tools Model & Schemas

### 2.1 Supported Tools
1. **Trendline** (`trendline`):
   - Defined by two points: $P_1 = (t_1, p_1)$ and $P_2 = (t_2, p_2)$.
   - Renders a straight vector or ray between the points with optional projection.
2. **Horizontal Level** (`horizontal_level`):
   - Defined by a single price coordinate $p$ and reference time $t$.
   - Extends across the entire chart width with a right-hand price badge.
3. **Fibonacci Retracement** (`fibonacci`):
   - Defined by two anchor points representing high and low: $P_{\text{start}} = (t_1, p_1)$ and $P_{\text{end}} = (t_2, p_2)$.
   - Renders horizontal ratios at: $0.0\%$, $23.6\%$, $38.2\%$, $50.0\%$, $61.8\%$, $78.6\%$, and $100.0\%$.
4. **Text Annotation** (`text`):
   - Defined by anchor point $P = (t, p)$ and text content string.

### 2.2 Database Schema (`chart_drawings`)
```sql
CREATE TABLE IF NOT EXISTS chart_drawings (
    id UUID PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    chart_id TEXT NOT NULL CHECK (length(chart_id) <= 128),
    dataset_version_id TEXT,
    tool TEXT NOT NULL CHECK (tool IN ('trendline', 'horizontal_level', 'fibonacci', 'text')),
    points JSONB NOT NULL,
    style JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_chart_drawings_lookup
    ON chart_drawings(workspace_id, chart_id, deleted_at);
```

### 2.3 Coordinates Representation
- Timestamp is represented in Unix nanoseconds (`ts_ns: i64`).
- Price is represented as a float64 (`price: f64`).

---

## 3. Saved Layout Templates Model

### 3.1 Database Schema (`chart_templates`)
```sql
CREATE TABLE IF NOT EXISTS chart_templates (
    id UUID PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0 AND length(name) <= 100),
    version INT NOT NULL DEFAULT 1 CHECK (version >= 1),
    indicators JSONB NOT NULL DEFAULT '[]'::jsonb,
    drawings JSONB NOT NULL DEFAULT '[]'::jsonb,
    comparisons JSONB NOT NULL DEFAULT '[]'::jsonb,
    dataset_versions JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX IF NOT EXISTS idx_chart_templates_workspace
    ON chart_templates(workspace_id, name);
```

---

## 4. API Specification

### 4.1 Drawing Endpoints
- `GET /api/v1/charts/{chart_id}/drawings`
  - Returns `{ "items": [DrawingView, ...] }` where `deleted_at IS NULL`.
- `POST /api/v1/charts/{chart_id}/drawings`
  - Body: `{ "tool": "...", "points": [...], "style": {...}, "dataset_version_id": "..." }`
  - Validates count $< 200$, payload $\le 64$ KB. Role $\ge$ editor.
  - Returns `201 Created` with created drawing.
- `PUT /api/v1/drawings/{id}`
  - Updates points/style. Role $\ge$ editor. Returns `200 OK`.
- `DELETE /api/v1/drawings/{id}`
  - Sets `deleted_at = now()`. Role $\ge$ editor. Returns `200 OK` `{ "deleted": true }`.

### 4.2 Comparison Endpoint
- `GET /api/v1/charts/compare?primary={symbol}&secondary={symbol}&timeframe={tf}&dataset_version={id}`
  - Validates secondary symbol in workspace datasets.
  - Validates timeframe matches native interval.
  - Applies sealed-holdout check.
  - Returns aligned timestamps, primary rebased values (base 100.0), secondary rebased values (base 100.0), and missing gaps as nulls.

### 4.3 Template Endpoints
- `GET /api/v1/chart-templates`
  - Lists templates in workspace.
- `POST /api/v1/chart-templates`
  - Creates template. Validates workspace limit $\le 50$.
- `GET /api/v1/chart-templates/{id}`
  - Returns template, checking for retired dataset versions.
- `DELETE /api/v1/chart-templates/{id}`
  - Deletes template. Role $\ge$ editor.
