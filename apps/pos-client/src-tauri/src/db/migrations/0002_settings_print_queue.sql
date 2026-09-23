-- v2: device-local settings and the offline receipt print queue.
-- Both are local-only (never synced): printers belong to this till.

CREATE TABLE settings (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  key TEXT NOT NULL UNIQUE,
  value TEXT NOT NULL CHECK (json_valid(value))
) STRICT;

-- One row per receipt to print. Rendered from the stored transaction at print
-- time, so a queued job always matches the immutable sale.
CREATE TABLE print_jobs (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  transaction_id TEXT NOT NULL REFERENCES transactions (id) DEFERRABLE INITIALLY DEFERRED,
  copy INTEGER NOT NULL CHECK (copy IN (0, 1)),
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  printed_at TEXT,
  last_error TEXT
) STRICT;
CREATE INDEX print_jobs_pending ON print_jobs (created_at) WHERE printed_at IS NULL AND deleted_at IS NULL;
CREATE INDEX print_jobs_transaction ON print_jobs (transaction_id);

CREATE TRIGGER settings_no_delete BEFORE DELETE ON settings BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER print_jobs_no_delete BEFORE DELETE ON print_jobs BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
