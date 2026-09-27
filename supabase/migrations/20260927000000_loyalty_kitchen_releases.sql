-- Phase 8.
--   * Mirrors of the tills' shop_settings and kitchen_tickets (see the local
--     migration 0005_loyalty_kitchen.sql), last-write-wins like the other
--     mutable rows.
--   * app_releases: the per-client update channel. The build workflow
--     uploads a signed installer to the private `releases` storage bucket
--     and records it with publish_app_release (service role only). The
--     `app-update` edge function asks app_update_check for the newest
--     release a till may install.

insert into public.sync_entities (entity_type, strategy, derived_columns) values
  ('shop_settings', 'last_write_wins', '{}'),
  ('kitchen_tickets', 'last_write_wins', '{}');

create table public.shop_settings (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  key text not null,
  value jsonb not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index shop_settings_client_seq on public.shop_settings (client_id, server_seq);
alter table public.shop_settings enable row level security;
revoke all on public.shop_settings from anon, authenticated;
create trigger shop_settings_no_delete before delete on public.shop_settings for each row execute function public.forbid_hard_delete();

create table public.kitchen_tickets (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  device_id uuid not null,
  ticket_number bigint not null,
  kind text not null,
  order_id uuid,
  transaction_id uuid,
  title text not null,
  order_type text not null,
  course bigint,
  server_name text not null,
  guests bigint not null,
  items jsonb not null,
  status text not null,
  fired_at timestamptz not null,
  ready_at timestamptz,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index kitchen_tickets_client_seq on public.kitchen_tickets (client_id, server_seq);
alter table public.kitchen_tickets enable row level security;
revoke all on public.kitchen_tickets from anon, authenticated;
create trigger kitchen_tickets_no_delete before delete on public.kitchen_tickets for each row execute function public.forbid_hard_delete();

-- ── Releases ──────────────────────────────────────────────────────────────

create table public.app_releases (
  id uuid primary key default gen_random_uuid(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  client_id uuid not null,
  version text not null check (version ~ '^[0-9]{1,5}\.[0-9]{1,5}\.[0-9]{1,9}$'),
  major integer not null,
  minor integer not null,
  patch integer not null,
  target text not null check (target in ('windows-x86_64')),
  notes text not null default '' check (char_length(notes) <= 4000),
  -- Object path in the `releases` bucket: <client_id>/<version>/<file>.
  storage_path text not null check (storage_path ~ '^[0-9a-f-]{36}/[0-9.]+/[A-Za-z0-9._ -]+$'),
  -- Minisign signature produced by `tauri build` (the `.sig` file).
  signature text not null check (char_length(signature) between 1 and 4096),
  published_at timestamptz not null default now(),
  -- A withdrawn release is no longer offered (tills that have it keep it).
  withdrawn_at timestamptz,
  unique (client_id, target, version)
);
create index app_releases_channel on public.app_releases (client_id, target, major, minor, patch)
  where deleted_at is null and withdrawn_at is null;
alter table public.app_releases enable row level security;
revoke all on public.app_releases from anon, authenticated;
create trigger app_releases_no_delete before delete on public.app_releases for each row execute function public.forbid_hard_delete();

-- Semantic version → sortable parts (MAJOR.MINOR.PATCH only).
create function public._version_parts(p_version text) returns integer[]
language plpgsql immutable as $$
begin
  if p_version is null or p_version !~ '^[0-9]{1,5}\.[0-9]{1,5}\.[0-9]{1,9}$' then
    raise exception 'invalid version %', coalesce(p_version, 'null') using errcode = '22023';
  end if;
  return string_to_array(p_version, '.')::integer[];
end $$;

-- Records (or re-records, idempotently) a published release. Called by the
-- build workflow with the service role after uploading the installer.
create function public.publish_app_release(
  p_client uuid, p_version text, p_target text, p_notes text, p_path text, p_signature text
) returns public.app_releases
language plpgsql as $$
declare
  v_parts integer[] := public._version_parts(p_version);
  v_row public.app_releases;
begin
  insert into public.app_releases (client_id, version, major, minor, patch, target, notes, storage_path, signature)
  values (p_client, p_version, v_parts[1], v_parts[2], v_parts[3], p_target, coalesce(p_notes, ''), p_path, p_signature)
  on conflict (client_id, target, version) do update
    set notes = excluded.notes, storage_path = excluded.storage_path, signature = excluded.signature,
        updated_at = now(), withdrawn_at = null
  returning * into v_row;
  return v_row;
end $$;
revoke all on function public.publish_app_release(uuid, text, text, text, text, text) from public, anon, authenticated;

-- The newest release above p_current for an activated, unrevoked till of
-- this client; no row = up to date. Raises 28000 like sync for a till the
-- cloud does not know.
create function public.app_update_check(
  p_client uuid, p_fingerprint text, p_current text, p_target text
) returns table (version text, notes text, published_at timestamptz, storage_path text, signature text)
language plpgsql as $$
declare
  v_current integer[] := public._version_parts(p_current);
  v_client public.client_licenses;
  v_activation public.device_activations;
begin
  select * into v_client from public.client_licenses where client_id = p_client;
  if not found or v_client.revoked_at is not null or v_client.deleted_at is not null then
    raise exception 'the license for this business is not active' using errcode = '28000';
  end if;
  select * into v_activation from public.device_activations
    where client_id = p_client and fingerprint_hash = p_fingerprint;
  if not found or v_activation.revoked_at is not null or v_activation.deleted_at is not null then
    raise exception 'this till is not activated with the cloud' using errcode = '28000';
  end if;
  return query
    select r.version, r.notes, r.published_at, r.storage_path, r.signature
    from public.app_releases r
    where r.client_id = p_client and r.target = p_target
      and r.deleted_at is null and r.withdrawn_at is null
      and array[r.major, r.minor, r.patch] > v_current
    order by r.major desc, r.minor desc, r.patch desc
    limit 1;
end $$;
revoke all on function public.app_update_check(uuid, text, text, text) from public, anon, authenticated;

-- The private bucket (Supabase only; a plain Postgres has no storage schema).
do $$
begin
  if exists (select 1 from information_schema.schemata where schema_name = 'storage') then
    insert into storage.buckets (id, name, public) values ('releases', 'releases', false)
      on conflict (id) do nothing;
  end if;
end $$;
