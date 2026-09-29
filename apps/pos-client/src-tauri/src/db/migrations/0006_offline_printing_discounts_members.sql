-- Phase 9: the kitchen print queue, discount schedules, memberships and the
-- shop-network (LAN) sync log.

-- ── Kitchen print queue (this till only, never synced) ──────────────────────
-- Tickets for the kitchen printer, kept until they print, like `print_jobs`
-- for receipts: a kitchen printer that is off or out of paper loses nothing.
CREATE TABLE kitchen_print_jobs (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  ticket TEXT NOT NULL CHECK (json_valid(ticket)),
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  printed_at TEXT,
  last_error TEXT
) STRICT;
CREATE INDEX kitchen_print_jobs_pending ON kitchen_print_jobs (created_at)
  WHERE printed_at IS NULL AND deleted_at IS NULL;
CREATE TRIGGER kitchen_print_jobs_no_delete BEFORE DELETE ON kitchen_print_jobs
BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
