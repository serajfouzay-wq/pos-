-- Cloud side of licensing: seat limits, device activations, revocation.
--
-- Only the `license-validate` Edge Function (service role) touches these
-- tables. RLS is enabled with NO policies, so the anon/authenticated roles a
-- till or browser could hold see nothing.
--
-- Rows are never hard-deleted (mirrors the local rule); revoke instead.

create table public.client_licenses (
  client_id uuid primary key,
  client_slug text not null,
  max_devices integer not null check (max_devices between 1 and 1000),
  -- iat of the token that last set max_devices: an older token cannot raise the limit back.
  limits_issued_at timestamptz not null,
  revoked_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz
);

create table public.device_activations (
  id uuid primary key default gen_random_uuid(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  client_id uuid not null references public.client_licenses (client_id),
  token_id uuid not null,
  fingerprint_hash text not null check (fingerprint_hash ~ '^[0-9a-f]{64}$'),
  device_name text not null check (char_length(device_name) between 1 and 120),
  activated_at timestamptz not null default now(),
  last_seen_at timestamptz,
  revoked_at timestamptz,
  unique (client_id, fingerprint_hash)
);

alter table public.client_licenses enable row level security;
alter table public.device_activations enable row level security;
revoke all on public.client_licenses, public.device_activations from anon, authenticated;

create function public.forbid_hard_delete() returns trigger
language plpgsql as $$
begin
  raise exception 'hard deletes are forbidden on %; set deleted_at / revoked_at', tg_table_name;
end $$;

create trigger client_licenses_no_delete before delete on public.client_licenses
  for each row execute function public.forbid_hard_delete();
create trigger device_activations_no_delete before delete on public.device_activations
  for each row execute function public.forbid_hard_delete();

-- Atomically records a validation. The caller has already verified the token
-- signature; claims passed here are therefore trusted.
--
-- Returns one of: active | revoked | device_limit.
create function public.validate_device_activation(
  p_client_id uuid,
  p_client_slug text,
  p_max_devices integer,
  p_token_issued_at timestamptz,
  p_token_id uuid,
  p_fingerprint text,
  p_device_name text
) returns table (status text, server_time timestamptz, reason text)
language plpgsql
security definer
set search_path = public
as $$
declare
  v_now timestamptz := now();
  v_client public.client_licenses;
  v_device public.device_activations;
  v_active integer;
begin
  -- Serialise per client so two new tills cannot both take the last seat.
  perform pg_advisory_xact_lock(hashtextextended(p_client_id::text, 0));

  insert into public.client_licenses (client_id, client_slug, max_devices, limits_issued_at)
  values (p_client_id, p_client_slug, p_max_devices, p_token_issued_at)
  on conflict (client_id) do update
    set max_devices = excluded.max_devices,
        client_slug = excluded.client_slug,
        limits_issued_at = excluded.limits_issued_at,
        updated_at = v_now
    where client_licenses.limits_issued_at < excluded.limits_issued_at;

  select * into v_client from public.client_licenses where client_id = p_client_id;
  if v_client.revoked_at is not null or v_client.deleted_at is not null then
    return query select 'revoked'::text, v_now, 'The license for this business has been revoked.'::text;
    return;
  end if;

  select * into v_device
  from public.device_activations
  where client_id = p_client_id and fingerprint_hash = p_fingerprint;

  if found then
    if v_device.revoked_at is not null or v_device.deleted_at is not null then
      return query select 'revoked'::text, v_now, 'This till has been deactivated.'::text;
      return;
    end if;
    update public.device_activations
      set last_seen_at = v_now, token_id = p_token_id, device_name = p_device_name, updated_at = v_now
      where id = v_device.id;
    return query select 'active'::text, v_now, null::text;
    return;
  end if;

  select count(*) into v_active
  from public.device_activations
  where client_id = p_client_id and revoked_at is null and deleted_at is null;

  if v_active >= v_client.max_devices then
    return query select 'device_limit'::text, v_now,
      format('All %s licensed tills are in use. Deactivate one first.', v_client.max_devices);
    return;
  end if;

  insert into public.device_activations (client_id, token_id, fingerprint_hash, device_name, last_seen_at)
  values (p_client_id, p_token_id, p_fingerprint, p_device_name, v_now);
  return query select 'active'::text, v_now, null::text;
end $$;

revoke execute on function public.validate_device_activation from public, anon, authenticated;
grant execute on function public.validate_device_activation to service_role;
