-- v3: business-type layouts (Phase 6).
--   * Modifier groups/options and which products ask them (cafe menus).
--   * Quick combos: fixed components sold together at a set price.
--   * Dining tables on a floor grid (restaurant table map).
--   * Open orders: cafe tabs and restaurant tables, held until paid (and
--     settled by one or more sales for split bills).
-- All synced, last-write-wins (see @pos/shared SYNC_ENTITY_STRATEGY).

CREATE TABLE modifier_groups (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  name TEXT NOT NULL,
  name_localized TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(name_localized)),
  min_select INTEGER NOT NULL CHECK (min_select BETWEEN 0 AND 20),
  max_select INTEGER NOT NULL CHECK (max_select BETWEEN 1 AND 20),
  sort_order INTEGER NOT NULL DEFAULT 0,
  is_active INTEGER NOT NULL CHECK (is_active IN (0, 1)),
  CHECK (min_select <= max_select)
) STRICT;

CREATE TABLE modifiers (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  group_id TEXT NOT NULL,
  name TEXT NOT NULL,
  name_localized TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(name_localized)),
  price_delta INTEGER NOT NULL,
  is_default INTEGER NOT NULL CHECK (is_default IN (0, 1)),
  sort_order INTEGER NOT NULL DEFAULT 0,
  is_active INTEGER NOT NULL CHECK (is_active IN (0, 1))
) STRICT;
CREATE INDEX modifiers_group ON modifiers (group_id);

CREATE TABLE product_modifier_groups (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  product_id TEXT NOT NULL,
  group_id TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE INDEX product_modifier_groups_product ON product_modifier_groups (product_id);

CREATE TABLE combos (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  name TEXT NOT NULL,
  name_localized TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(name_localized)),
  price INTEGER NOT NULL CHECK (price >= 0),
  color TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  is_active INTEGER NOT NULL CHECK (is_active IN (0, 1))
) STRICT;

CREATE TABLE combo_items (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  combo_id TEXT NOT NULL,
  product_id TEXT NOT NULL,
  quantity_milli INTEGER NOT NULL CHECK (quantity_milli > 0),
  sort_order INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE INDEX combo_items_combo ON combo_items (combo_id);

CREATE TABLE dining_tables (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  label TEXT NOT NULL,
  area TEXT NOT NULL DEFAULT '',
  seats INTEGER NOT NULL CHECK (seats BETWEEN 1 AND 50),
  shape TEXT NOT NULL CHECK (shape IN ('square', 'round', 'bar')),
  grid_x INTEGER NOT NULL CHECK (grid_x BETWEEN 0 AND 23),
  grid_y INTEGER NOT NULL CHECK (grid_y BETWEEN 0 AND 15),
  sort_order INTEGER NOT NULL DEFAULT 0,
  is_active INTEGER NOT NULL CHECK (is_active IN (0, 1))
) STRICT;

CREATE TABLE open_orders (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  device_id TEXT NOT NULL,
  order_type TEXT NOT NULL CHECK (order_type IN ('counter', 'dine_in', 'takeaway', 'delivery')),
  table_id TEXT,
  label TEXT,
  guests INTEGER NOT NULL DEFAULT 0 CHECK (guests BETWEEN 0 AND 99),
  status TEXT NOT NULL CHECK (status IN ('open', 'settled', 'cancelled')),
  items TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(items)),
  transaction_ids TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(transaction_ids)),
  opened_by TEXT NOT NULL,
  opened_at TEXT NOT NULL,
  closed_at TEXT,
  notes TEXT
) STRICT;
CREATE INDEX open_orders_open ON open_orders (status, table_id) WHERE deleted_at IS NULL;

CREATE TRIGGER modifier_groups_no_delete BEFORE DELETE ON modifier_groups BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER modifiers_no_delete BEFORE DELETE ON modifiers BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER product_modifier_groups_no_delete BEFORE DELETE ON product_modifier_groups BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER combos_no_delete BEFORE DELETE ON combos BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER combo_items_no_delete BEFORE DELETE ON combo_items BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER dining_tables_no_delete BEFORE DELETE ON dining_tables BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER open_orders_no_delete BEFORE DELETE ON open_orders BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
