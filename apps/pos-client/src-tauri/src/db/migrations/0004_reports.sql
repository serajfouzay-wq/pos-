-- v4: reports (Phase 7).
--   * z_reports: end-of-day closings, numbered per till, append-only and
--     synced. `report` is the snapshot that was printed.
--   * Indexes for history, refunds against a sale, per-till periods and the
--     audit trail filters.

CREATE TABLE z_reports (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT CHECK (deleted_at IS NULL),
  device_id TEXT NOT NULL,
  z_number INTEGER NOT NULL CHECK (z_number > 0),
  period_start TEXT NOT NULL,
  period_end TEXT NOT NULL,
  run_by TEXT NOT NULL,
  currency TEXT NOT NULL CHECK (length(currency) = 3),
  sale_count INTEGER NOT NULL CHECK (sale_count >= 0),
  refund_count INTEGER NOT NULL CHECK (refund_count >= 0),
  void_count INTEGER NOT NULL CHECK (void_count >= 0),
  gross_sales INTEGER NOT NULL CHECK (gross_sales >= 0),
  discount_total INTEGER NOT NULL CHECK (discount_total >= 0),
  refund_total INTEGER NOT NULL CHECK (refund_total >= 0),
  void_total INTEGER NOT NULL CHECK (void_total >= 0),
  net_sales INTEGER NOT NULL,
  tax_total INTEGER NOT NULL,
  grand_total INTEGER NOT NULL,
  report TEXT NOT NULL CHECK (json_valid(report)),
  CHECK (period_start <= period_end),
  UNIQUE (device_id, z_number)
) STRICT;

CREATE TRIGGER z_reports_no_delete BEFORE DELETE ON z_reports BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER z_reports_append_only BEFORE UPDATE ON z_reports BEGIN SELECT RAISE(ABORT, 'z_reports is append-only'); END;

CREATE INDEX transactions_original ON transactions (original_transaction_id) WHERE original_transaction_id IS NOT NULL;
CREATE INDEX transactions_device_occurred ON transactions (device_id, occurred_at);
CREATE INDEX transaction_items_transaction ON transaction_items (transaction_id);
CREATE INDEX audit_log_action ON audit_log (action, occurred_at);
CREATE INDEX shifts_device_opened ON shifts (device_id, opened_at);
