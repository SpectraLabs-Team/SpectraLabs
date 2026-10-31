-- Add version column to strategy_drafts for optimistic concurrency control (M25 Task 6b).
ALTER TABLE strategy_drafts
    ADD COLUMN IF NOT EXISTS version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0);
