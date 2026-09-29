-- Phase 9.
--   * Discount schedules: how a rule applies (manual/automatic), on which
--     days and in which local time window. Nullable, so tills that are not
--     updated yet keep syncing (NULL = manual, every day, all day).
--   * Mirrors of membership_plans and memberships (last-write-wins), see
--     the local migration 0006_offline_printing_discounts_members.sql.

alter table public.discount_rules
  add column apply_mode text check (apply_mode is null or apply_mode in ('manual', 'automatic')),
  add column days_mask bigint check (days_mask is null or days_mask between 1 and 127),
  add column time_from bigint check (time_from is null or time_from between 0 and 1439),
  add column time_to bigint check (time_to is null or time_to between 1 and 1440);

insert into public.sync_entities (entity_type, strategy, derived_columns) values
  ('membership_plans', 'last_write_wins', '{}'),
  ('memberships', 'last_write_wins', '{}');

create table public.membership_plans (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  name text not null,
  description text,
  product_id uuid not null,
  price bigint not null,
  duration_days bigint not null,
  discount_bps bigint not null,
  points_multiplier_bps bigint not null,
  color text,
  is_active boolean not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index membership_plans_client_seq on public.membership_plans (client_id, server_seq);
alter table public.membership_plans enable row level security;
revoke all on public.membership_plans from anon, authenticated;
create trigger membership_plans_no_delete before delete on public.membership_plans for each row execute function public.forbid_hard_delete();

create table public.memberships (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  customer_id uuid not null,
  plan_id uuid not null,
  card_number text not null,
  starts_at timestamptz not null,
  ends_at timestamptz not null,
  status text not null,
  transaction_id uuid,
  price_paid bigint not null,
  device_id uuid not null,
  notes text,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index memberships_client_seq on public.memberships (client_id, server_seq);
alter table public.memberships enable row level security;
revoke all on public.memberships from anon, authenticated;
create trigger memberships_no_delete before delete on public.memberships for each row execute function public.forbid_hard_delete();
