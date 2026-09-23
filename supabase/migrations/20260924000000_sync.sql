-- Cloud mirror of the POS databases + the sync protocol (push / pull).
--
-- Every synced local table has a mirror here with four server-only columns:
--   client_id        tenant (from the verified license, never from the payload)
--   server_seq       position in the global change sequence (pull cursor)
--   origin_device_id device whose event produced the current version
--   last_event_id    that event (LWW tie-breaker: (updated_at, last_event_id))
-- plus received_at. Column sets are pinned to packages/shared/contracts/db-schema.json
-- by supabase/tests/sync.test.sql.
--
-- ALL writes go through _sync_apply under the per-client advisory lock, so
-- server_seq commits in order per client and a pull cursor can never skip a
-- row that commits later. No foreign keys: devices may push children and
-- parents in separate batches; integrity is owned by the device.

alter table public.device_activations add column device_id uuid;

create sequence public.sync_seq;

create table public.sync_entities (
  entity_type text primary key,
  strategy text not null check (strategy in ('last_write_wins', 'append_only', 'additive_delta')),
  -- Aggregates derived server-side from additive tables; never taken from a device row.
  derived_columns text[] not null default '{}'
);

-- Idempotency: an event id is applied at most once, however often it is replayed.
create table public.sync_received_events (
  event_id uuid primary key,
  client_id uuid not null,
  device_id uuid not null,
  entity_type text not null,
  received_at timestamptz not null default now()
);

insert into public.sync_entities (entity_type, strategy, derived_columns) values
  ('categories', 'last_write_wins', '{}'),
  ('products', 'last_write_wins', '{stock_on_hand_milli}'),
  ('customers', 'last_write_wins', '{loyalty_points}'),
  ('users', 'last_write_wins', '{}'),
  ('discount_rules', 'last_write_wins', '{}'),
  ('shifts', 'last_write_wins', '{}'),
  ('suppliers', 'last_write_wins', '{}'),
  ('purchase_orders', 'last_write_wins', '{}'),
  ('purchase_order_items', 'last_write_wins', '{}'),
  ('transactions', 'append_only', '{}'),
  ('transaction_items', 'append_only', '{}'),
  ('transaction_payments', 'append_only', '{}'),
  ('audit_log', 'append_only', '{}'),
  ('stock_movements', 'additive_delta', '{}'),
  ('loyalty_ledger', 'additive_delta', '{}');

create table public.categories (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  name text not null,
  name_localized jsonb not null,
  parent_id uuid,
  sort_order bigint not null,
  color text,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index categories_client_seq on public.categories (client_id, server_seq);
alter table public.categories enable row level security;
revoke all on public.categories from anon, authenticated;
create trigger categories_no_delete before delete on public.categories for each row execute function public.forbid_hard_delete();

create table public.products (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  name text not null,
  name_localized jsonb not null,
  category_id uuid,
  sku text,
  barcode text,
  price bigint not null,
  cost bigint,
  tax_rate_bps bigint not null,
  unit text not null,
  sold_by_weight boolean not null,
  track_stock boolean not null,
  stock_on_hand_milli bigint not null,
  reorder_threshold_milli bigint,
  reorder_quantity_milli bigint,
  image_asset text,
  quick_key_position bigint,
  is_active boolean not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index products_client_seq on public.products (client_id, server_seq);
alter table public.products enable row level security;
revoke all on public.products from anon, authenticated;
create trigger products_no_delete before delete on public.products for each row execute function public.forbid_hard_delete();

create table public.customers (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  display_name text not null,
  phone text,
  email text,
  loyalty_points bigint not null,
  notes text,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index customers_client_seq on public.customers (client_id, server_seq);
alter table public.customers enable row level security;
revoke all on public.customers from anon, authenticated;
create trigger customers_no_delete before delete on public.customers for each row execute function public.forbid_hard_delete();

create table public.users (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  display_name text not null,
  role text not null,
  pin_hash text not null,
  is_active boolean not null,
  failed_pin_attempts bigint not null,
  locked_until timestamptz,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index users_client_seq on public.users (client_id, server_seq);
alter table public.users enable row level security;
revoke all on public.users from anon, authenticated;
create trigger users_no_delete before delete on public.users for each row execute function public.forbid_hard_delete();

create table public.discount_rules (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  name text not null,
  kind text not null,
  value bigint not null,
  scope text not null,
  target_id uuid,
  min_subtotal bigint,
  starts_at timestamptz,
  ends_at timestamptz,
  is_active boolean not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index discount_rules_client_seq on public.discount_rules (client_id, server_seq);
alter table public.discount_rules enable row level security;
revoke all on public.discount_rules from anon, authenticated;
create trigger discount_rules_no_delete before delete on public.discount_rules for each row execute function public.forbid_hard_delete();

create table public.shifts (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  device_id uuid not null,
  opened_by uuid not null,
  closed_by uuid,
  opened_at timestamptz not null,
  closed_at timestamptz,
  opening_float bigint not null,
  closing_float bigint,
  expected_cash bigint,
  actual_cash bigint,
  variance bigint,
  notes text,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index shifts_client_seq on public.shifts (client_id, server_seq);
alter table public.shifts enable row level security;
revoke all on public.shifts from anon, authenticated;
create trigger shifts_no_delete before delete on public.shifts for each row execute function public.forbid_hard_delete();

create table public.suppliers (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  name text not null,
  contact_name text,
  phone text,
  email text,
  tax_number text,
  notes text,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index suppliers_client_seq on public.suppliers (client_id, server_seq);
alter table public.suppliers enable row level security;
revoke all on public.suppliers from anon, authenticated;
create trigger suppliers_no_delete before delete on public.suppliers for each row execute function public.forbid_hard_delete();

create table public.purchase_orders (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  supplier_id uuid not null,
  reference text not null,
  status text not null,
  currency text not null,
  total bigint not null,
  ordered_at timestamptz,
  expected_at timestamptz,
  created_by uuid not null,
  notes text,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index purchase_orders_client_seq on public.purchase_orders (client_id, server_seq);
alter table public.purchase_orders enable row level security;
revoke all on public.purchase_orders from anon, authenticated;
create trigger purchase_orders_no_delete before delete on public.purchase_orders for each row execute function public.forbid_hard_delete();

create table public.purchase_order_items (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  purchase_order_id uuid not null,
  product_id uuid not null,
  product_name text not null,
  quantity_ordered_milli bigint not null,
  quantity_received_milli bigint not null,
  unit_cost bigint not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index purchase_order_items_client_seq on public.purchase_order_items (client_id, server_seq);
alter table public.purchase_order_items enable row level security;
revoke all on public.purchase_order_items from anon, authenticated;
create trigger purchase_order_items_no_delete before delete on public.purchase_order_items for each row execute function public.forbid_hard_delete();

create table public.transactions (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  kind text not null,
  original_transaction_id uuid,
  receipt_number text not null,
  device_id uuid not null,
  shift_id uuid not null,
  cashier_id uuid not null,
  approved_by uuid,
  customer_id uuid,
  order_type text not null,
  table_label text,
  currency text not null,
  subtotal bigint not null,
  discount_total bigint not null,
  tax_total bigint not null,
  total bigint not null,
  loyalty_points_earned bigint not null,
  loyalty_points_redeemed bigint not null,
  notes text,
  idempotency_key uuid not null,
  occurred_at timestamptz not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index transactions_client_seq on public.transactions (client_id, server_seq);
alter table public.transactions enable row level security;
revoke all on public.transactions from anon, authenticated;
create trigger transactions_no_delete before delete on public.transactions for each row execute function public.forbid_hard_delete();

create table public.transaction_items (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  transaction_id uuid not null,
  line_number bigint not null,
  product_id uuid not null,
  product_name text not null,
  sku text,
  unit_price bigint not null,
  quantity_milli bigint not null,
  modifiers jsonb not null,
  discount_amount bigint not null,
  tax_rate_bps bigint not null,
  tax_amount bigint not null,
  line_total bigint not null,
  course bigint,
  note text,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index transaction_items_client_seq on public.transaction_items (client_id, server_seq);
alter table public.transaction_items enable row level security;
revoke all on public.transaction_items from anon, authenticated;
create trigger transaction_items_no_delete before delete on public.transaction_items for each row execute function public.forbid_hard_delete();

create table public.transaction_payments (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  transaction_id uuid not null,
  method text not null,
  amount bigint not null,
  tendered_currency text not null,
  tendered_amount bigint not null,
  rate_numerator bigint,
  rate_denominator bigint,
  change_given bigint not null,
  reference text,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index transaction_payments_client_seq on public.transaction_payments (client_id, server_seq);
alter table public.transaction_payments enable row level security;
revoke all on public.transaction_payments from anon, authenticated;
create trigger transaction_payments_no_delete before delete on public.transaction_payments for each row execute function public.forbid_hard_delete();

create table public.audit_log (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  user_id uuid not null,
  role text not null,
  action text not null,
  entity_type text not null,
  entity_id uuid,
  before jsonb,
  after jsonb,
  device_id uuid not null,
  occurred_at timestamptz not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index audit_log_client_seq on public.audit_log (client_id, server_seq);
alter table public.audit_log enable row level security;
revoke all on public.audit_log from anon, authenticated;
create trigger audit_log_no_delete before delete on public.audit_log for each row execute function public.forbid_hard_delete();

create table public.stock_movements (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  product_id uuid not null,
  quantity_delta_milli bigint not null,
  reason text not null,
  reference_id uuid,
  device_id uuid not null,
  user_id uuid not null,
  occurred_at timestamptz not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index stock_movements_client_seq on public.stock_movements (client_id, server_seq);
alter table public.stock_movements enable row level security;
revoke all on public.stock_movements from anon, authenticated;
create trigger stock_movements_no_delete before delete on public.stock_movements for each row execute function public.forbid_hard_delete();

create table public.loyalty_ledger (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  customer_id uuid not null,
  transaction_id uuid,
  points_delta bigint not null,
  reason text not null,
  device_id uuid not null,
  user_id uuid not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index loyalty_ledger_client_seq on public.loyalty_ledger (client_id, server_seq);
alter table public.loyalty_ledger enable row level security;
revoke all on public.loyalty_ledger from anon, authenticated;
create trigger loyalty_ledger_no_delete before delete on public.loyalty_ledger for each row execute function public.forbid_hard_delete();

-- ── Append-only guards ─────────────────────────────────────────────────────

create function public.forbid_update() returns trigger
language plpgsql as $$
begin
  raise exception '% is append-only', tg_table_name;
end $$;

create trigger transactions_append_only before update on public.transactions for each row execute function public.forbid_update();
create trigger transaction_items_append_only before update on public.transaction_items for each row execute function public.forbid_update();
create trigger transaction_payments_append_only before update on public.transaction_payments for each row execute function public.forbid_update();
create trigger audit_log_append_only before update on public.audit_log for each row execute function public.forbid_update();
create trigger stock_movements_append_only before update on public.stock_movements for each row execute function public.forbid_update();
create trigger loyalty_ledger_append_only before update on public.loyalty_ledger for each row execute function public.forbid_update();

-- ── Derived aggregates (additive deltas) ──────────────────────────────────
-- Stock on hand and loyalty balances are sums of their delta tables. A new
-- product/customer starts from the deltas already received (they may arrive
-- first); each new delta adds to the aggregate. LWW upserts never overwrite
-- these columns (see sync_entities.derived_columns).

create function public._sync_products_init_stock() returns trigger
language plpgsql as $$
begin
  new.stock_on_hand_milli := coalesce(
    (select sum(quantity_delta_milli) from public.stock_movements
      where client_id = new.client_id and product_id = new.id), 0);
  return new;
end $$;
create trigger products_init_stock before insert on public.products
  for each row execute function public._sync_products_init_stock();

create function public._sync_apply_stock_movement() returns trigger
language plpgsql as $$
begin
  update public.products set stock_on_hand_milli = stock_on_hand_milli + new.quantity_delta_milli
    where client_id = new.client_id and id = new.product_id;
  return null;
end $$;
create trigger stock_movements_apply after insert on public.stock_movements
  for each row execute function public._sync_apply_stock_movement();

create function public._sync_customers_init_points() returns trigger
language plpgsql as $$
begin
  new.loyalty_points := coalesce(
    (select sum(points_delta) from public.loyalty_ledger
      where client_id = new.client_id and customer_id = new.id), 0);
  return new;
end $$;
create trigger customers_init_points before insert on public.customers
  for each row execute function public._sync_customers_init_points();

create function public._sync_apply_loyalty() returns trigger
language plpgsql as $$
begin
  update public.customers set loyalty_points = loyalty_points + new.points_delta
    where client_id = new.client_id and id = new.customer_id;
  return null;
end $$;
create trigger loyalty_ledger_apply after insert on public.loyalty_ledger
  for each row execute function public._sync_apply_loyalty();

-- ── Helpers ────────────────────────────────────────────────────────────────

-- The protocol's fixed timestamp format (UTC, milliseconds, Z): lexical order
-- equals time order on the device.
create function public._sync_ts(p timestamptz) returns text
language sql immutable as $$
  select to_char(p at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
$$;

create function public._sync_columns(p_table text) returns table (name text, type regtype)
language sql stable as $$
  select attname::text, atttypid::regtype from pg_attribute
  where attrelid = ('public.' || quote_ident(p_table))::regclass and attnum > 0 and not attisdropped
  order by attnum
$$;

-- Errors are raised with SQLSTATE 28000 (authorization) so the edge function
-- can answer 403; the device then re-validates its license.
create function public._sync_authorize(p_client uuid, p_fingerprint text, p_device uuid) returns void
language plpgsql as $$
declare
  v_activation public.device_activations;
  v_client public.client_licenses;
begin
  select * into v_client from public.client_licenses where client_id = p_client;
  if not found or v_client.revoked_at is not null or v_client.deleted_at is not null then
    raise exception 'the license for this business is not active' using errcode = '28000';
  end if;
  select * into v_activation from public.device_activations
    where client_id = p_client and fingerprint_hash = p_fingerprint for update;
  if not found then
    raise exception 'this till is not activated with the cloud yet' using errcode = '28000';
  end if;
  if v_activation.revoked_at is not null or v_activation.deleted_at is not null then
    raise exception 'this till has been deactivated' using errcode = '28000';
  end if;
  -- The device key (checked by the edge function) already proves this is the
  -- activated hardware. A different device id therefore means the till's
  -- local database was recreated (reinstall, wiped disk): rebind, so the new
  -- database bootstraps everything — including rows the old one pushed.
  if v_activation.device_id is distinct from p_device then
    update public.device_activations set device_id = p_device, updated_at = now() where id = v_activation.id;
  end if;
end $$;

-- Applies one event. Returns 'applied', 'stale' (an LWW version that lost) or
-- 'duplicate' (already received). Raises on invalid events (22xxx / 23xxx =
-- permanent: the device parks them; anything else is retryable).
create function public._sync_apply(p_client uuid, p_device uuid, p_event jsonb) returns text
language plpgsql as $$
declare
  v_entity text := p_event->>'entity_type';
  v_event uuid := (p_event->>'event_id')::uuid;
  v_payload jsonb := p_event->'payload';
  v_strategy text;
  v_derived text[];
  v_row jsonb;
  v_set text;
  v_count integer;
begin
  select strategy, derived_columns into v_strategy, v_derived from public.sync_entities where entity_type = v_entity;
  if not found then
    raise exception 'unknown entity type %', v_entity using errcode = '22023';
  end if;
  if jsonb_typeof(v_payload) is distinct from 'object' or (v_payload->>'id') is distinct from (p_event->>'entity_id') then
    raise exception 'payload.id must equal entity_id' using errcode = '22023';
  end if;
  if (p_event->>'event_type') is distinct from (case when v_strategy = 'last_write_wins' then 'upsert' else 'append' end) then
    raise exception 'event_type % does not match % for %', p_event->>'event_type', v_strategy, v_entity using errcode = '22023';
  end if;

  insert into public.sync_received_events (event_id, client_id, device_id, entity_type)
    values (v_event, p_client, p_device, v_entity) on conflict do nothing;
  get diagnostics v_count = row_count;
  if v_count = 0 then
    return 'duplicate';
  end if;

  -- Server-owned columns always come from the server, never the payload.
  v_row := v_payload || jsonb_build_object(
    'client_id', p_client,
    'server_seq', nextval('public.sync_seq'),
    'origin_device_id', p_device,
    'last_event_id', v_event,
    'received_at', now());

  if v_strategy = 'last_write_wins' then
    select string_agg(format('%1$I = excluded.%1$I', c.name), ', ') into v_set
      from public._sync_columns(v_entity) c
      where c.name not in ('id', 'client_id', 'created_at') and c.name <> all (v_derived);
    -- The client_id guard means a device can never overwrite another tenant's row.
    execute format(
      'insert into public.%1$I as t select * from jsonb_populate_record(null::public.%1$I, $1)
       on conflict (id) do update set %2$s
       where t.client_id = excluded.client_id
         and (excluded.updated_at, excluded.last_event_id) > (t.updated_at, t.last_event_id)',
      v_entity, v_set) using v_row;
  else
    execute format(
      'insert into public.%1$I select * from jsonb_populate_record(null::public.%1$I, $1)
       on conflict (id) do nothing',
      v_entity) using v_row;
  end if;
  get diagnostics v_count = row_count;
  return case when v_count = 1 then 'applied' else 'stale' end;
end $$;

-- ── Protocol entry points (called by the sync-push / sync-pull functions) ──

create function public.sync_push(p_client uuid, p_fingerprint text, p_device uuid, p_events jsonb) returns jsonb
language plpgsql security definer set search_path = public as $$
declare
  v_event jsonb;
  v_ack jsonb := '[]';
  v_rejected jsonb := '[]';
begin
  perform public._sync_authorize(p_client, p_fingerprint, p_device);
  if jsonb_typeof(p_events) is distinct from 'array' or jsonb_array_length(p_events) > 500 then
    raise exception 'events must be an array of at most 500' using errcode = '22023';
  end if;
  -- Serialise this client's writes: server_seq then commits in order.
  perform pg_advisory_xact_lock(hashtextextended(p_client::text, 4242));
  for v_event in select value from jsonb_array_elements(p_events) loop
    begin
      perform public._sync_apply(p_client, p_device, v_event);
      v_ack := v_ack || to_jsonb(v_event->>'event_id');
    exception when others then
      v_rejected := v_rejected || jsonb_build_object(
        'event_id', v_event->>'event_id',
        'reason', sqlerrm,
        'retryable', not (sqlstate like '22%' or sqlstate like '23%'));
    end;
  end loop;
  return jsonb_build_object('acknowledged', v_ack, 'rejected', v_rejected, 'server_time', public._sync_ts(now()));
end $$;

-- Changes for this client after `p_cursor`, oldest first, excluding versions
-- this device produced itself. Timestamps are rendered in protocol format.
create function public.sync_pull(p_client uuid, p_fingerprint text, p_device uuid, p_cursor bigint, p_limit integer)
returns jsonb
language plpgsql security definer set search_path = public as $$
declare
  v_entity public.sync_entities;
  v_parts text[] := '{}';
  v_max_parts text[] := '{}';
  v_json text;
  v_changes jsonb;
  v_count integer;
  v_next bigint;
  v_max bigint;
  v_limit integer := least(greatest(coalesce(p_limit, 500), 1), 1000);
begin
  perform public._sync_authorize(p_client, p_fingerprint, p_device);
  for v_entity in select * from public.sync_entities order by entity_type loop
    select 'jsonb_build_object(' || string_agg(
             format('%L, %s', c.name, case when c.type = 'timestamptz'::regtype
                                           then format('public._sync_ts(%I)', c.name)
                                           else quote_ident(c.name) end), ', ') || ')'
      into v_json
      from public._sync_columns(v_entity.entity_type) c
      where c.name not in ('client_id', 'server_seq', 'origin_device_id', 'last_event_id', 'received_at');
    v_parts := v_parts || format(
      'select %L::text as entity_type, server_seq, %s as event_id, %s as row
         from public.%I where client_id = $1 and server_seq > $2 and origin_device_id is distinct from $3',
      v_entity.entity_type,
      case when v_entity.strategy = 'last_write_wins' then 'last_event_id' else 'null::uuid' end,
      v_json, v_entity.entity_type);
    v_max_parts := v_max_parts || format(
      'select max(server_seq) as m from public.%I where client_id = $1', v_entity.entity_type);
  end loop;

  execute format(
    'select coalesce(jsonb_agg(jsonb_build_object(''entity_type'', entity_type, ''row'', row, ''event_id'', event_id)
                               order by server_seq), ''[]''), count(*), max(server_seq)
       from (select * from (%s) u where server_seq > $2 order by server_seq limit $4) page',
    array_to_string(v_parts, ' union all '))
    into v_changes, v_count, v_next
    using p_client, coalesce(p_cursor, 0), p_device, v_limit;

  if v_count < v_limit then
    -- Nothing else visible for this device: skip past its own versions too.
    execute format('select max(m) from (%s) x', array_to_string(v_max_parts, ' union all '))
      into v_max using p_client;
    v_next := greatest(coalesce(v_next, 0), coalesce(v_max, 0), coalesce(p_cursor, 0));
  end if;

  return jsonb_build_object(
    'changes', v_changes,
    'next_cursor', coalesce(v_next, p_cursor, 0)::text,
    'has_more', v_count >= v_limit);
end $$;

revoke execute on function public.sync_push(uuid, text, uuid, jsonb) from public, anon, authenticated;
revoke execute on function public.sync_pull(uuid, text, uuid, bigint, integer) from public, anon, authenticated;
grant execute on function public.sync_push(uuid, text, uuid, jsonb) to service_role;
grant execute on function public.sync_pull(uuid, text, uuid, bigint, integer) to service_role;
