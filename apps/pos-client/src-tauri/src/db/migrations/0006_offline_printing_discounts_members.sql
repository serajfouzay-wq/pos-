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

-- ── Discount schedules ──────────────────────────────────────────────────────
-- NULL keeps the Phase 1 meaning (tills not yet updated sync rows without
-- these columns): a manual discount, every day, all day.
ALTER TABLE discount_rules ADD COLUMN apply_mode TEXT
  CHECK (apply_mode IS NULL OR apply_mode IN ('manual', 'automatic'));
ALTER TABLE discount_rules ADD COLUMN days_mask INTEGER
  CHECK (days_mask IS NULL OR days_mask BETWEEN 1 AND 127);
ALTER TABLE discount_rules ADD COLUMN time_from INTEGER
  CHECK (time_from IS NULL OR time_from BETWEEN 0 AND 1439);
ALTER TABLE discount_rules ADD COLUMN time_to INTEGER
  CHECK (time_to IS NULL OR time_to BETWEEN 1 AND 1440);

-- ── Memberships ─────────────────────────────────────────────────────────────
-- A plan is sold like any product (its own product row, so payments,
-- receipts, reports and refunds all work unchanged); selling it to a
-- customer starts or extends their membership.
CREATE TABLE membership_plans (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
  description TEXT,
  product_id TEXT NOT NULL,
  price INTEGER NOT NULL CHECK (price >= 0),
  duration_days INTEGER NOT NULL CHECK (duration_days BETWEEN 1 AND 3660),
  discount_bps INTEGER NOT NULL CHECK (discount_bps BETWEEN 0 AND 10000),
  points_multiplier_bps INTEGER NOT NULL DEFAULT 10000
    CHECK (points_multiplier_bps BETWEEN 0 AND 100000),
  color TEXT,
  is_active INTEGER NOT NULL CHECK (is_active IN (0, 1))
) STRICT;
CREATE TRIGGER membership_plans_no_delete BEFORE DELETE ON membership_plans
BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;

CREATE TABLE memberships (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  customer_id TEXT NOT NULL,
  plan_id TEXT NOT NULL,
  card_number TEXT NOT NULL CHECK (length(card_number) BETWEEN 4 AND 32),
  starts_at TEXT NOT NULL,
  ends_at TEXT NOT NULL CHECK (ends_at > starts_at),
  status TEXT NOT NULL CHECK (status IN ('active', 'cancelled')),
  -- The sale that paid for it (NULL when the owner granted it).
  transaction_id TEXT,
  price_paid INTEGER NOT NULL CHECK (price_paid >= 0),
  device_id TEXT NOT NULL,
  notes TEXT
) STRICT;
CREATE INDEX memberships_customer ON memberships (customer_id, ends_at);
CREATE INDEX memberships_card ON memberships (card_number);
CREATE INDEX memberships_transaction ON memberships (transaction_id);
CREATE TRIGGER memberships_no_delete BEFORE DELETE ON memberships
BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;

-- ── Shop network (LAN) hub ──────────────────────────────────────────────────
-- On the till chosen as the hub, the shop's shared state as the other tills
-- see it: the same rules as the cloud (last-write-wins by (updated_at,
-- event id), insert-once appends, a change sequence for pulls). Local only.
CREATE TABLE hub_rows (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL CHECK (length(entity_id) = 36),
  row TEXT NOT NULL CHECK (json_valid(row)),
  seq INTEGER NOT NULL,
  origin_device_id TEXT NOT NULL,
  -- The event that wrote this version (last-write-wins rows only).
  event_id TEXT,
  UNIQUE (entity_type, entity_id)
) STRICT;
CREATE INDEX hub_rows_seq ON hub_rows (seq);
CREATE TRIGGER hub_rows_no_delete BEFORE DELETE ON hub_rows
BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;

-- Events already applied (a till retrying after a lost answer is not applied twice).
CREATE TABLE hub_events (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  device_id TEXT NOT NULL
) STRICT;
CREATE TRIGGER hub_events_no_delete BEFORE DELETE ON hub_events
BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
