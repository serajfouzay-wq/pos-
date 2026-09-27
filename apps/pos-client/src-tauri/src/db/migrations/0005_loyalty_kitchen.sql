-- v5: Phase 8.
--   * shop_settings: settings shared by the shop's tills (loyalty programme),
--     synced last-write-wins. Fixed row id per key (see SHOP_SETTING_IDS).
--   * kitchen_tickets: what the kitchen display shows, synced
--     last-write-wins (the kitchen bumps, the till created it).
--   * Indexes for customer lookups and customer history.

CREATE TABLE shop_settings (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  key TEXT NOT NULL UNIQUE,
  value TEXT NOT NULL CHECK (json_valid(value))
) STRICT;

CREATE TABLE kitchen_tickets (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  device_id TEXT NOT NULL,
  ticket_number INTEGER NOT NULL CHECK (ticket_number > 0),
  kind TEXT NOT NULL CHECK (kind IN ('order', 'void')),
  order_id TEXT,
  transaction_id TEXT,
  title TEXT NOT NULL,
  order_type TEXT NOT NULL CHECK (order_type IN ('counter', 'dine_in', 'takeaway', 'delivery')),
  course INTEGER CHECK (course BETWEEN 1 AND 9),
  server_name TEXT NOT NULL,
  guests INTEGER NOT NULL DEFAULT 0 CHECK (guests BETWEEN 0 AND 99),
  items TEXT NOT NULL CHECK (json_valid(items)),
  status TEXT NOT NULL CHECK (status IN ('open', 'ready')),
  fired_at TEXT NOT NULL,
  ready_at TEXT
) STRICT;
CREATE INDEX kitchen_tickets_board ON kitchen_tickets (status, fired_at) WHERE deleted_at IS NULL;
CREATE INDEX kitchen_tickets_device ON kitchen_tickets (device_id, ticket_number);

CREATE TRIGGER shop_settings_no_delete BEFORE DELETE ON shop_settings BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER kitchen_tickets_no_delete BEFORE DELETE ON kitchen_tickets BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;

CREATE INDEX customers_name ON customers (display_name COLLATE NOCASE) WHERE deleted_at IS NULL;
CREATE INDEX transactions_customer ON transactions (customer_id) WHERE customer_id IS NOT NULL;
CREATE INDEX loyalty_ledger_transaction ON loyalty_ledger (transaction_id) WHERE transaction_id IS NOT NULL;
