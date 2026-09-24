-- Phase 6: mirrors of the menu, floor and open-order tables (see the local
-- migration 0003_menu_tables_orders.sql). Same shape as the other LWW
-- mirrors: the device's columns + client_id, server_seq, origin_device_id,
-- last_event_id, received_at. The generic sync functions pick them up from
-- sync_entities; nothing else changes.

insert into public.sync_entities (entity_type, strategy, derived_columns) values
  ('modifier_groups', 'last_write_wins', '{}'),
  ('modifiers', 'last_write_wins', '{}'),
  ('product_modifier_groups', 'last_write_wins', '{}'),
  ('combos', 'last_write_wins', '{}'),
  ('combo_items', 'last_write_wins', '{}'),
  ('dining_tables', 'last_write_wins', '{}'),
  ('open_orders', 'last_write_wins', '{}');

create table public.modifier_groups (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  name text not null,
  name_localized jsonb not null,
  min_select bigint not null,
  max_select bigint not null,
  sort_order bigint not null,
  is_active boolean not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index modifier_groups_client_seq on public.modifier_groups (client_id, server_seq);
alter table public.modifier_groups enable row level security;
revoke all on public.modifier_groups from anon, authenticated;
create trigger modifier_groups_no_delete before delete on public.modifier_groups for each row execute function public.forbid_hard_delete();

create table public.modifiers (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  group_id uuid not null,
  name text not null,
  name_localized jsonb not null,
  price_delta bigint not null,
  is_default boolean not null,
  sort_order bigint not null,
  is_active boolean not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index modifiers_client_seq on public.modifiers (client_id, server_seq);
alter table public.modifiers enable row level security;
revoke all on public.modifiers from anon, authenticated;
create trigger modifiers_no_delete before delete on public.modifiers for each row execute function public.forbid_hard_delete();

create table public.product_modifier_groups (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  product_id uuid not null,
  group_id uuid not null,
  sort_order bigint not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index product_modifier_groups_client_seq on public.product_modifier_groups (client_id, server_seq);
alter table public.product_modifier_groups enable row level security;
revoke all on public.product_modifier_groups from anon, authenticated;
create trigger product_modifier_groups_no_delete before delete on public.product_modifier_groups for each row execute function public.forbid_hard_delete();

create table public.combos (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  name text not null,
  name_localized jsonb not null,
  price bigint not null,
  color text,
  sort_order bigint not null,
  is_active boolean not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index combos_client_seq on public.combos (client_id, server_seq);
alter table public.combos enable row level security;
revoke all on public.combos from anon, authenticated;
create trigger combos_no_delete before delete on public.combos for each row execute function public.forbid_hard_delete();

create table public.combo_items (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  combo_id uuid not null,
  product_id uuid not null,
  quantity_milli bigint not null,
  sort_order bigint not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index combo_items_client_seq on public.combo_items (client_id, server_seq);
alter table public.combo_items enable row level security;
revoke all on public.combo_items from anon, authenticated;
create trigger combo_items_no_delete before delete on public.combo_items for each row execute function public.forbid_hard_delete();

create table public.dining_tables (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  label text not null,
  area text not null,
  seats bigint not null,
  shape text not null,
  grid_x bigint not null,
  grid_y bigint not null,
  sort_order bigint not null,
  is_active boolean not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index dining_tables_client_seq on public.dining_tables (client_id, server_seq);
alter table public.dining_tables enable row level security;
revoke all on public.dining_tables from anon, authenticated;
create trigger dining_tables_no_delete before delete on public.dining_tables for each row execute function public.forbid_hard_delete();

create table public.open_orders (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  device_id uuid not null,
  order_type text not null,
  table_id uuid,
  label text,
  guests bigint not null,
  status text not null,
  items jsonb not null,
  transaction_ids jsonb not null,
  opened_by uuid not null,
  opened_at timestamptz not null,
  closed_at timestamptz,
  notes text,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index open_orders_client_seq on public.open_orders (client_id, server_seq);
alter table public.open_orders enable row level security;
revoke all on public.open_orders from anon, authenticated;
create trigger open_orders_no_delete before delete on public.open_orders for each row execute function public.forbid_hard_delete();
