-- M1 Data Studio, task 12: point-in-time data, symbol changes, delistings, and survivorship-free universes.

-- Symbol changes: tracks ticker renames over time.
CREATE TABLE symbol_changes (
    id              uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id    uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    old_symbol      text NOT NULL CHECK (length(btrim(old_symbol)) > 0),
    new_symbol      text NOT NULL CHECK (length(btrim(new_symbol)) > 0),
    effective_date  timestamptz NOT NULL,
    notes           text NOT NULL DEFAULT '',
    created_at      timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, old_symbol, effective_date),
    UNIQUE (workspace_id, id)
);
CREATE INDEX symbol_changes_ws_old ON symbol_changes (workspace_id, old_symbol, effective_date);
CREATE INDEX symbol_changes_ws_new ON symbol_changes (workspace_id, new_symbol, effective_date);

-- Instrument delistings: tracks when symbols cease trading.
CREATE TABLE instrument_delistings (
    id              uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id    uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    symbol          text NOT NULL CHECK (length(btrim(symbol)) > 0),
    delisted_at     timestamptz NOT NULL,
    reason          text NOT NULL DEFAULT '',
    created_at      timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, symbol, delisted_at),
    UNIQUE (workspace_id, id)
);
CREATE INDEX instrument_delistings_ws_sym ON instrument_delistings (workspace_id, symbol, delisted_at);

-- Universes: named collections of instruments.
CREATE TABLE universes (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         text NOT NULL CHECK (length(btrim(name)) > 0),
    description  text NOT NULL DEFAULT '',
    created_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, name),
    UNIQUE (workspace_id, id)
);

-- Universe members: symbol constituents with addition/removal intervals.
CREATE TABLE universe_members (
    id           uuid PRIMARY KEY DEFAULT spectra_uuidv7(),
    workspace_id uuid NOT NULL,
    universe_id  uuid NOT NULL,
    symbol       text NOT NULL CHECK (length(btrim(symbol)) > 0),
    added_at     timestamptz NOT NULL DEFAULT now(),
    removed_at   timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (workspace_id, universe_id) REFERENCES universes (workspace_id, id) ON DELETE CASCADE,
    UNIQUE (workspace_id, universe_id, symbol, added_at)
);
CREATE INDEX universe_members_lookup ON universe_members (universe_id, symbol, added_at);
