-- POS client local schema, v1.
--
-- Conventions (mirrors @pos/shared; pinned by packages/shared/contracts/db-schema.json):
--   * STRICT tables: SQLite rejects a REAL in an INTEGER column, so a float can
--     never be stored as money.
--   * id TEXT PRIMARY KEY (UUID); created_at / updated_at / deleted_at on every
--     table as UTC ISO-8601 text with milliseconds (lexically sortable).
--   * Money: INTEGER minor units. Quantities: INTEGER thousandths (*_milli).
--     Rates: INTEGER basis points. Booleans: INTEGER 0/1. JSON: TEXT + json_valid.
--   * Soft deletes only: a trigger on every table aborts DELETE.
--   * Append-only tables additionally abort UPDATE.
--   * Foreign keys only inside an aggregate (a transaction and its lines), and
--     DEFERRABLE, so rows pulled by sync in any order within a batch still apply.
--     Cross-aggregate references are validated by the Rust layer.

-- ── Identity & licensing (local only) ─────────────────────────────────────

CREATE TABLE license (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  license_id TEXT NOT NULL UNIQUE,
  client_id TEXT NOT NULL,
  token TEXT NOT NULL,
  fingerprint_hash TEXT NOT NULL CHECK (length(fingerprint_hash) = 64),
  issued_at TEXT NOT NULL,
  expires_at TEXT,
  last_seen_at TEXT,
  revoked_at TEXT,
  clock_high_water_at TEXT
) STRICT;

CREATE TABLE device (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  name TEXT NOT NULL
) STRICT;

-- ── People ────────────────────────────────────────────────────────────────

CREATE TABLE users (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  display_name TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('owner', 'manager', 'cashier')),
  pin_hash TEXT NOT NULL,
  is_active INTEGER NOT NULL CHECK (is_active IN (0, 1)),
  failed_pin_attempts INTEGER NOT NULL DEFAULT 0 CHECK (failed_pin_attempts >= 0),
  locked_until TEXT
) STRICT;

CREATE TABLE customers (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  display_name TEXT NOT NULL,
  phone TEXT,
  email TEXT,
  loyalty_points INTEGER NOT NULL DEFAULT 0,
  notes TEXT
) STRICT;
CREATE INDEX customers_phone ON customers (phone) WHERE deleted_at IS NULL;

CREATE TABLE loyalty_ledger (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  customer_id TEXT NOT NULL,
  transaction_id TEXT,
  points_delta INTEGER NOT NULL CHECK (points_delta <> 0),
  reason TEXT NOT NULL CHECK (reason IN ('earn', 'redeem', 'adjust', 'expire', 'refund_reversal')),
  device_id TEXT NOT NULL,
  user_id TEXT NOT NULL
) STRICT;
CREATE INDEX loyalty_ledger_customer ON loyalty_ledger (customer_id);

-- ── Catalogue & inventory ─────────────────────────────────────────────────

CREATE TABLE categories (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  name TEXT NOT NULL,
  name_localized TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(name_localized)),
  parent_id TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  color TEXT
) STRICT;

CREATE TABLE products (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  name TEXT NOT NULL,
  name_localized TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(name_localized)),
  category_id TEXT,
  sku TEXT,
  barcode TEXT,
  price INTEGER NOT NULL CHECK (price >= 0),
  cost INTEGER CHECK (cost >= 0),
  tax_rate_bps INTEGER NOT NULL CHECK (tax_rate_bps BETWEEN 0 AND 10000),
  unit TEXT NOT NULL CHECK (unit IN ('each', 'kg', 'g', 'l', 'ml')),
  sold_by_weight INTEGER NOT NULL CHECK (sold_by_weight IN (0, 1)),
  track_stock INTEGER NOT NULL CHECK (track_stock IN (0, 1)),
  stock_on_hand_milli INTEGER NOT NULL DEFAULT 0,
  reorder_threshold_milli INTEGER CHECK (reorder_threshold_milli >= 0),
  reorder_quantity_milli INTEGER CHECK (reorder_quantity_milli > 0),
  image_asset TEXT,
  quick_key_position INTEGER CHECK (quick_key_position >= 0),
  is_active INTEGER NOT NULL CHECK (is_active IN (0, 1))
) STRICT;
CREATE INDEX products_barcode ON products (barcode) WHERE deleted_at IS NULL;
CREATE INDEX products_sku ON products (sku) WHERE deleted_at IS NULL;
CREATE INDEX products_category ON products (category_id);

CREATE TABLE stock_movements (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  product_id TEXT NOT NULL,
  quantity_delta_milli INTEGER NOT NULL CHECK (quantity_delta_milli <> 0),
  reason TEXT NOT NULL CHECK (reason IN ('sale', 'refund', 'purchase_receipt', 'adjustment', 'waste', 'stock_count')),
  reference_id TEXT,
  device_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  occurred_at TEXT NOT NULL
) STRICT;
CREATE INDEX stock_movements_product ON stock_movements (product_id);

CREATE TABLE discount_rules (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('percentage', 'fixed_amount')),
  value INTEGER NOT NULL CHECK (value >= 0),
  scope TEXT NOT NULL CHECK (scope IN ('order', 'product', 'category')),
  target_id TEXT,
  min_subtotal INTEGER CHECK (min_subtotal >= 0),
  starts_at TEXT,
  ends_at TEXT,
  is_active INTEGER NOT NULL CHECK (is_active IN (0, 1)),
  CHECK (kind <> 'percentage' OR value <= 10000),
  CHECK ((scope = 'order') = (target_id IS NULL))
) STRICT;

-- ── Sales ─────────────────────────────────────────────────────────────────

CREATE TABLE shifts (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  device_id TEXT NOT NULL,
  opened_by TEXT NOT NULL,
  closed_by TEXT,
  opened_at TEXT NOT NULL,
  closed_at TEXT,
  opening_float INTEGER NOT NULL CHECK (opening_float >= 0),
  closing_float INTEGER CHECK (closing_float >= 0),
  expected_cash INTEGER,
  actual_cash INTEGER CHECK (actual_cash >= 0),
  variance INTEGER,
  notes TEXT
) STRICT;
-- At most one open shift per till.
CREATE UNIQUE INDEX shifts_one_open_per_device ON shifts (device_id) WHERE closed_at IS NULL AND deleted_at IS NULL;

CREATE TABLE transactions (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT CHECK (deleted_at IS NULL),
  kind TEXT NOT NULL CHECK (kind IN ('sale', 'refund', 'void')),
  original_transaction_id TEXT,
  receipt_number TEXT NOT NULL,
  device_id TEXT NOT NULL,
  shift_id TEXT NOT NULL,
  cashier_id TEXT NOT NULL,
  approved_by TEXT,
  customer_id TEXT,
  order_type TEXT NOT NULL CHECK (order_type IN ('counter', 'dine_in', 'takeaway', 'delivery')),
  table_label TEXT,
  currency TEXT NOT NULL CHECK (length(currency) = 3),
  subtotal INTEGER NOT NULL,
  discount_total INTEGER NOT NULL,
  tax_total INTEGER NOT NULL,
  total INTEGER NOT NULL,
  loyalty_points_earned INTEGER NOT NULL CHECK (loyalty_points_earned >= 0),
  loyalty_points_redeemed INTEGER NOT NULL CHECK (loyalty_points_redeemed >= 0),
  notes TEXT,
  idempotency_key TEXT NOT NULL UNIQUE,
  occurred_at TEXT NOT NULL,
  CHECK ((kind = 'sale') = (original_transaction_id IS NULL)),
  UNIQUE (device_id, receipt_number)
) STRICT;
CREATE INDEX transactions_shift ON transactions (shift_id);
CREATE INDEX transactions_occurred ON transactions (occurred_at);

CREATE TABLE transaction_items (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT CHECK (deleted_at IS NULL),
  transaction_id TEXT NOT NULL REFERENCES transactions (id) DEFERRABLE INITIALLY DEFERRED,
  line_number INTEGER NOT NULL CHECK (line_number > 0),
  product_id TEXT NOT NULL,
  product_name TEXT NOT NULL,
  sku TEXT,
  unit_price INTEGER NOT NULL CHECK (unit_price >= 0),
  quantity_milli INTEGER NOT NULL CHECK (quantity_milli > 0),
  modifiers TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(modifiers)),
  discount_amount INTEGER NOT NULL CHECK (discount_amount >= 0),
  tax_rate_bps INTEGER NOT NULL CHECK (tax_rate_bps BETWEEN 0 AND 10000),
  tax_amount INTEGER NOT NULL CHECK (tax_amount >= 0),
  line_total INTEGER NOT NULL CHECK (line_total >= 0),
  course INTEGER CHECK (course > 0),
  note TEXT,
  UNIQUE (transaction_id, line_number)
) STRICT;

CREATE TABLE transaction_payments (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT CHECK (deleted_at IS NULL),
  transaction_id TEXT NOT NULL REFERENCES transactions (id) DEFERRABLE INITIALLY DEFERRED,
  method TEXT NOT NULL CHECK (method IN ('cash', 'card', 'wallet', 'loyalty', 'voucher')),
  amount INTEGER NOT NULL,
  tendered_currency TEXT NOT NULL CHECK (length(tendered_currency) = 3),
  tendered_amount INTEGER NOT NULL,
  rate_numerator INTEGER CHECK (rate_numerator > 0),
  rate_denominator INTEGER CHECK (rate_denominator > 0),
  change_given INTEGER NOT NULL CHECK (change_given >= 0),
  reference TEXT,
  CHECK ((rate_numerator IS NULL) = (rate_denominator IS NULL))
) STRICT;
CREATE INDEX transaction_payments_transaction ON transaction_payments (transaction_id);

-- ── Purchasing ────────────────────────────────────────────────────────────

CREATE TABLE suppliers (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  name TEXT NOT NULL,
  contact_name TEXT,
  phone TEXT,
  email TEXT,
  tax_number TEXT,
  notes TEXT
) STRICT;

CREATE TABLE purchase_orders (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  supplier_id TEXT NOT NULL,
  reference TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('draft', 'sent', 'partially_received', 'received', 'cancelled')),
  currency TEXT NOT NULL CHECK (length(currency) = 3),
  total INTEGER NOT NULL CHECK (total >= 0),
  ordered_at TEXT,
  expected_at TEXT,
  created_by TEXT NOT NULL,
  notes TEXT
) STRICT;

CREATE TABLE purchase_order_items (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  purchase_order_id TEXT NOT NULL REFERENCES purchase_orders (id) DEFERRABLE INITIALLY DEFERRED,
  product_id TEXT NOT NULL,
  product_name TEXT NOT NULL,
  quantity_ordered_milli INTEGER NOT NULL CHECK (quantity_ordered_milli > 0),
  quantity_received_milli INTEGER NOT NULL DEFAULT 0 CHECK (quantity_received_milli >= 0),
  unit_cost INTEGER NOT NULL CHECK (unit_cost >= 0)
) STRICT;

-- ── Audit & sync ──────────────────────────────────────────────────────────

CREATE TABLE audit_log (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT CHECK (deleted_at IS NULL),
  user_id TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('owner', 'manager', 'cashier')),
  action TEXT NOT NULL,
  entity_type TEXT NOT NULL,
  entity_id TEXT,
  before TEXT CHECK (before IS NULL OR json_valid(before)),
  after TEXT CHECK (after IS NULL OR json_valid(after)),
  device_id TEXT NOT NULL,
  occurred_at TEXT NOT NULL
) STRICT;
CREATE INDEX audit_log_occurred ON audit_log (occurred_at);

CREATE TABLE sync_queue (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  event_type TEXT NOT NULL CHECK (event_type IN ('upsert', 'append')),
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  payload TEXT NOT NULL CHECK (json_valid(payload)),
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  sent_at TEXT,
  next_attempt_at TEXT,
  last_error TEXT
) STRICT;
CREATE INDEX sync_queue_pending ON sync_queue (created_at) WHERE sent_at IS NULL AND deleted_at IS NULL;

-- ── Invariants ────────────────────────────────────────────────────────────
-- Generated list; keep in step with the tables above (the Rust migration test
-- asserts every table has a no-delete trigger and every append-only table a
-- no-update trigger).

CREATE TRIGGER license_no_delete BEFORE DELETE ON license BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER device_no_delete BEFORE DELETE ON device BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER users_no_delete BEFORE DELETE ON users BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER customers_no_delete BEFORE DELETE ON customers BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER loyalty_ledger_no_delete BEFORE DELETE ON loyalty_ledger BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER categories_no_delete BEFORE DELETE ON categories BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER products_no_delete BEFORE DELETE ON products BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER stock_movements_no_delete BEFORE DELETE ON stock_movements BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER discount_rules_no_delete BEFORE DELETE ON discount_rules BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER shifts_no_delete BEFORE DELETE ON shifts BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER transactions_no_delete BEFORE DELETE ON transactions BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER transaction_items_no_delete BEFORE DELETE ON transaction_items BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER transaction_payments_no_delete BEFORE DELETE ON transaction_payments BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER suppliers_no_delete BEFORE DELETE ON suppliers BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER purchase_orders_no_delete BEFORE DELETE ON purchase_orders BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER purchase_order_items_no_delete BEFORE DELETE ON purchase_order_items BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER sync_queue_no_delete BEFORE DELETE ON sync_queue BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;

CREATE TRIGGER transactions_append_only BEFORE UPDATE ON transactions BEGIN SELECT RAISE(ABORT, 'transactions are append-only'); END;
CREATE TRIGGER transaction_items_append_only BEFORE UPDATE ON transaction_items BEGIN SELECT RAISE(ABORT, 'transaction_items are append-only'); END;
CREATE TRIGGER transaction_payments_append_only BEFORE UPDATE ON transaction_payments BEGIN SELECT RAISE(ABORT, 'transaction_payments are append-only'); END;
CREATE TRIGGER stock_movements_append_only BEFORE UPDATE ON stock_movements BEGIN SELECT RAISE(ABORT, 'stock_movements are append-only'); END;
CREATE TRIGGER loyalty_ledger_append_only BEFORE UPDATE ON loyalty_ledger BEGIN SELECT RAISE(ABORT, 'loyalty_ledger is append-only'); END;
CREATE TRIGGER audit_log_append_only BEFORE UPDATE ON audit_log BEGIN SELECT RAISE(ABORT, 'audit_log is append-only'); END;
