-- Server-side sync tests. Run on a fresh database after the migrations:
--   psql -v ON_ERROR_STOP=1 -v schema="$(cat packages/shared/contracts/db-schema.json)" -f supabase/tests/sync.test.sql
-- Every block ASSERTs; the first failure aborts the run.

\set ON_ERROR_STOP 1

-- Fixtures: two tenants, three activated tills.
create temp table t_ids as select
  '11111111-1111-4111-8111-111111111111'::uuid as client_a,
  '22222222-2222-4222-8222-222222222222'::uuid as client_b,
  'a1a1a1a1-0000-4000-8000-000000000001'::uuid as dev_a1,
  'a1a1a1a1-0000-4000-8000-000000000002'::uuid as dev_a2,
  'b1b1b1b1-0000-4000-8000-000000000001'::uuid as dev_b1,
  repeat('a1', 32) as fp_a1, repeat('a2', 32) as fp_a2, repeat('b1', 32) as fp_b1;

select public.validate_device_activation(client_a, 'a', 5, now(), gen_random_uuid(), fp_a1, 'A1') from t_ids;
select public.validate_device_activation(client_a, 'a', 5, now(), gen_random_uuid(), fp_a2, 'A2') from t_ids;
select public.validate_device_activation(client_b, 'b', 5, now(), gen_random_uuid(), fp_b1, 'B1') from t_ids;

create function pg_temp.product(p_id uuid, p_updated text, p_price bigint, p_stock bigint default 999) returns jsonb
language sql as $$
  select jsonb_build_object(
    'id', p_id, 'created_at', '2026-09-24T08:00:00.000Z', 'updated_at', p_updated, 'deleted_at', null,
    'name', 'Latte', 'name_localized', '{}'::jsonb, 'category_id', null, 'sku', null, 'barcode', null,
    'price', p_price, 'cost', null, 'tax_rate_bps', 0, 'unit', 'each', 'sold_by_weight', false,
    'track_stock', true, 'stock_on_hand_milli', p_stock, 'reorder_threshold_milli', null,
    'reorder_quantity_milli', null, 'image_asset', null, 'quick_key_position', null, 'is_active', true)
$$;

create function pg_temp.event(p_event uuid, p_type text, p_entity text, p_payload jsonb) returns jsonb
language sql as $$
  select jsonb_build_object('event_id', p_event, 'device_id', null, 'event_type', p_type, 'entity_type', p_entity,
    'entity_id', p_payload->>'id', 'payload', p_payload, 'occurred_at', '2026-09-24T08:00:00.000Z')
$$;

create function pg_temp.movement(p_id uuid, p_product uuid, p_delta bigint, p_device uuid) returns jsonb
language sql as $$
  select jsonb_build_object('id', p_id, 'created_at', '2026-09-24T08:00:00.000Z', 'updated_at', '2026-09-24T08:00:00.000Z',
    'deleted_at', null, 'product_id', p_product, 'quantity_delta_milli', p_delta, 'reason', 'sale', 'reference_id', null,
    'device_id', p_device, 'user_id', p_device, 'occurred_at', '2026-09-24T08:00:00.000Z')
$$;

-- 1. Authorization ─────────────────────────────────────────────────────────
do $$
declare ids record := (select t from t_ids t);
begin
  begin
    perform public.sync_push(ids.client_a, repeat('ff', 32), ids.dev_a1, '[]');
    raise exception 'unactivated fingerprint was accepted';
  exception when sqlstate '28000' then null;
  end;
  -- First contact binds the device id to the activation…
  perform public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1, '[]');
  perform public.sync_push(ids.client_a, ids.fp_a2, ids.dev_a2, '[]');
  perform public.sync_push(ids.client_b, ids.fp_b1, ids.dev_b1, '[]');
  -- A fresh local database on the same hardware (new device id) rebinds.
  perform public.sync_push(ids.client_a, ids.fp_a1, '00000000-0000-4000-8000-0000000000ff', '[]');
  assert (select device_id from device_activations where fingerprint_hash = ids.fp_a1)
    = '00000000-0000-4000-8000-0000000000ff', 'rebound to the new device id';
  perform public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1, '[]');
  assert (select device_id from device_activations where fingerprint_hash = ids.fp_a1) = ids.dev_a1, 'rebound back';
  raise notice 'ok 1 authorization';
end $$;

-- 2. LWW upsert, pull visibility and format ────────────────────────────────
do $$
declare
  ids record := (select t from t_ids t);
  p uuid := '00000000-0000-4000-8000-00000000a001';
  r jsonb;
  pulled jsonb;
begin
  r := public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1, jsonb_build_array(
    pg_temp.event('e0000000-0000-4000-8000-000000000001', 'upsert', 'products', pg_temp.product(p, '2026-09-24T09:00:00.000Z', 1250))));
  assert jsonb_array_length(r->'acknowledged') = 1 and jsonb_array_length(r->'rejected') = 0, r::text;
  assert r->>'server_time' ~ '^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$', r->>'server_time';

  pulled := public.sync_pull(ids.client_a, ids.fp_a2, ids.dev_a2, 0, 100);
  assert jsonb_array_length(pulled->'changes') = 1, pulled::text;
  assert pulled->'changes'->0->'row'->>'updated_at' = '2026-09-24T09:00:00.000Z', 'timestamps in protocol format';
  assert pulled->'changes'->0->>'event_id' = 'e0000000-0000-4000-8000-000000000001', 'LWW tie-breaker exposed';
  assert (pulled->'changes'->0->'row') ? 'client_id' = false, 'server columns stripped';
  assert (pulled->'changes'->0->'row'->>'stock_on_hand_milli')::bigint = 0, 'stock is server-derived, not the device cache';

  -- The originating device does not get its own version back.
  pulled := public.sync_pull(ids.client_a, ids.fp_a1, ids.dev_a1, 0, 100);
  assert jsonb_array_length(pulled->'changes') = 0, pulled::text;
  assert (pulled->>'next_cursor')::bigint > 0, 'cursor skips past own versions';
  raise notice 'ok 2 upsert + pull';
end $$;

-- 3. LWW conflict resolution ───────────────────────────────────────────────
do $$
declare
  ids record := (select t from t_ids t);
  p uuid := '00000000-0000-4000-8000-00000000a001';
  price bigint;
begin
  -- Older write loses (acknowledged, but not applied).
  perform public.sync_push(ids.client_a, ids.fp_a2, ids.dev_a2, jsonb_build_array(
    pg_temp.event('e0000000-0000-4000-8000-000000000002', 'upsert', 'products', pg_temp.product(p, '2026-09-24T08:30:00.000Z', 1))));
  select products.price into price from products where id = p;
  assert price = 1250, 'older write must not win';
  -- Newer write wins.
  perform public.sync_push(ids.client_a, ids.fp_a2, ids.dev_a2, jsonb_build_array(
    pg_temp.event('e0000000-0000-4000-8000-000000000003', 'upsert', 'products', pg_temp.product(p, '2026-09-24T10:00:00.000Z', 1500))));
  select products.price into price from products where id = p;
  assert price = 1500, 'newer write must win';
  -- Same timestamp: the higher event id wins, deterministically.
  perform public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1, jsonb_build_array(
    pg_temp.event('e0000000-0000-4000-8000-000000000001', 'upsert', 'products', pg_temp.product(p, '2026-09-24T10:00:00.000Z', 7))));
  select products.price into price from products where id = p;
  assert price = 1500, 'duplicate event id is ignored';
  perform public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1, jsonb_build_array(
    pg_temp.event('00000000-0000-4000-8000-000000000009', 'upsert', 'products', pg_temp.product(p, '2026-09-24T10:00:00.000Z', 8))));
  select products.price into price from products where id = p;
  assert price = 1500, 'tie with a lower event id loses';
  perform public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1, jsonb_build_array(
    pg_temp.event('ffffffff-0000-4000-8000-000000000009', 'upsert', 'products', pg_temp.product(p, '2026-09-24T10:00:00.000Z', 1600))));
  select products.price into price from products where id = p;
  assert price = 1600, 'tie with a higher event id wins';
  raise notice 'ok 3 last-write-wins';
end $$;

-- 4. Additive stock deltas, idempotent under replay ────────────────────────
do $$
declare
  ids record := (select t from t_ids t);
  p uuid := '00000000-0000-4000-8000-00000000a001';
  late uuid := '00000000-0000-4000-8000-00000000a002';
  m1 jsonb := pg_temp.movement('00000000-0000-4000-8000-0000000b0001', p, -1000, ids.dev_a1);
  m2 jsonb := pg_temp.movement('00000000-0000-4000-8000-0000000b0002', p, -2000, ids.dev_a2);
  stock bigint;
begin
  perform public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1,
    jsonb_build_array(pg_temp.event('e0000000-0000-4000-8000-0000000000b1', 'append', 'stock_movements', m1)));
  -- Replayed batch (e.g. ack lost in transit): applied once.
  perform public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1,
    jsonb_build_array(pg_temp.event('e0000000-0000-4000-8000-0000000000b1', 'append', 'stock_movements', m1)));
  perform public.sync_push(ids.client_a, ids.fp_a2, ids.dev_a2,
    jsonb_build_array(pg_temp.event('e0000000-0000-4000-8000-0000000000b2', 'append', 'stock_movements', m2)));
  select stock_on_hand_milli into stock from products where id = p;
  assert stock = -3000, format('two tills selling offline both count, got %s', stock);

  -- A product upsert never overwrites the derived stock.
  perform public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1, jsonb_build_array(
    pg_temp.event('e0000000-0000-4000-8000-0000000000c1', 'upsert', 'products', pg_temp.product(p, '2026-09-24T11:00:00.000Z', 1700, 123456))));
  select stock_on_hand_milli into stock from products where id = p;
  assert stock = -3000, 'LWW upsert kept the derived stock';

  -- Movements that arrive before their product are counted when it arrives.
  perform public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1, jsonb_build_array(
    pg_temp.event('e0000000-0000-4000-8000-0000000000c2', 'append', 'stock_movements',
      pg_temp.movement('00000000-0000-4000-8000-0000000b0003', late, 5000, ids.dev_a1)),
    pg_temp.event('e0000000-0000-4000-8000-0000000000c3', 'upsert', 'products', pg_temp.product(late, '2026-09-24T11:00:00.000Z', 100))));
  select stock_on_hand_milli into stock from products where id = late;
  assert stock = 5000, format('late product starts from prior movements, got %s', stock);
  raise notice 'ok 4 additive deltas';
end $$;

-- 5. Tenant isolation ──────────────────────────────────────────────────────
do $$
declare
  ids record := (select t from t_ids t);
  p uuid := '00000000-0000-4000-8000-00000000a001';
  price bigint;
  pulled jsonb;
begin
  -- Tenant B tries to overwrite tenant A's product by reusing its id.
  perform public.sync_push(ids.client_b, ids.fp_b1, ids.dev_b1, jsonb_build_array(
    pg_temp.event('e0000000-0000-4000-8000-0000000000d1', 'upsert', 'products', pg_temp.product(p, '2099-01-01T00:00:00.000Z', 1))));
  select products.price into price from products where id = p and client_id = ids.client_a;
  assert price = 1700, 'another tenant can never overwrite a row';
  pulled := public.sync_pull(ids.client_b, ids.fp_b1, ids.dev_b1, 0, 100);
  assert jsonb_array_length(pulled->'changes') = 0, 'tenants never see each other''s data';
  -- A's fingerprint cannot be used under B's client id.
  begin
    perform public.sync_pull(ids.client_b, ids.fp_a1, ids.dev_a1, 0, 100);
    raise exception 'cross-tenant fingerprint accepted';
  exception when sqlstate '28000' then null;
  end;
  raise notice 'ok 5 tenant isolation';
end $$;

-- 6. Append-only rows and permanent rejections ─────────────────────────────
do $$
declare
  ids record := (select t from t_ids t);
  r jsonb;
  bad jsonb;
begin
  -- Wrong event type for the table's strategy.
  r := public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1, jsonb_build_array(
    pg_temp.event('e0000000-0000-4000-8000-0000000000e1', 'upsert', 'stock_movements',
      pg_temp.movement('00000000-0000-4000-8000-0000000b0009', gen_random_uuid(), 1000, ids.dev_a1))));
  assert jsonb_array_length(r->'rejected') = 1 and (r->'rejected'->0->>'retryable')::boolean = false, r::text;
  -- Unknown entity.
  r := public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1, jsonb_build_array(
    jsonb_build_object('event_id', 'e0000000-0000-4000-8000-0000000000e2', 'event_type', 'upsert', 'entity_type', 'license',
      'entity_id', gen_random_uuid(), 'payload', jsonb_build_object('id', gen_random_uuid()))));
  assert (r->'rejected'->0->>'retryable')::boolean = false, r::text;
  -- Missing NOT NULL column → constraint violation (23502) → permanent.
  bad := pg_temp.product(gen_random_uuid(), '2026-09-24T09:00:00.000Z', 1) - 'name';
  r := public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1, jsonb_build_array(
    pg_temp.event('e0000000-0000-4000-8000-0000000000e3', 'upsert', 'products', bad)));
  assert (r->'rejected'->0->>'retryable')::boolean = false, r::text;
  -- A rejected event is not recorded as received: a corrected retry can still apply.
  assert not exists (select 1 from sync_received_events where event_id = 'e0000000-0000-4000-8000-0000000000e3');
  -- One bad event does not sink the batch.
  r := public.sync_push(ids.client_a, ids.fp_a1, ids.dev_a1, jsonb_build_array(
    pg_temp.event('e0000000-0000-4000-8000-0000000000e4', 'upsert', 'products', bad),
    pg_temp.event('e0000000-0000-4000-8000-0000000000e5', 'upsert', 'products',
      pg_temp.product(gen_random_uuid(), '2026-09-24T09:00:00.000Z', 1))));
  assert jsonb_array_length(r->'acknowledged') = 1 and jsonb_array_length(r->'rejected') = 1, r::text;
  -- Append-only tables refuse updates even server-side.
  begin
    update stock_movements set quantity_delta_milli = 0;
    raise exception 'append-only update allowed';
  exception when raise_exception then
    if sqlerrm not like '%append-only%' then raise; end if;
  end;
  raise notice 'ok 6 rejections + append-only';
end $$;

-- 7. Pagination ────────────────────────────────────────────────────────────
do $$
declare
  ids record := (select t from t_ids t);
  page jsonb;
  total integer := 0;
  cursor_ bigint := 0;
  pages integer := 0;
begin
  loop
    page := public.sync_pull(ids.client_a, ids.fp_a2, ids.dev_a2, cursor_, 2);
    total := total + jsonb_array_length(page->'changes');
    cursor_ := (page->>'next_cursor')::bigint;
    pages := pages + 1;
    exit when not (page->>'has_more')::boolean;
  end loop;
  -- Device A2 sees everything A1 produced (and nothing it produced itself).
  assert total = (select count(*) from (
      select id from products where client_id = ids.client_a and origin_device_id <> ids.dev_a2
      union all select id from stock_movements where client_id = ids.client_a and origin_device_id <> ids.dev_a2) x),
    format('pagination returned %s rows', total);
  assert pages >= 2, 'more than one page';
  assert jsonb_array_length((public.sync_pull(ids.client_a, ids.fp_a2, ids.dev_a2, cursor_, 2))->'changes') = 0,
    'nothing left after the final cursor';
  raise notice 'ok 7 pagination';
end $$;

-- 8. Revocation ────────────────────────────────────────────────────────────
do $$
declare ids record := (select t from t_ids t);
begin
  update device_activations set revoked_at = now() where fingerprint_hash = ids.fp_a2;
  begin
    perform public.sync_pull(ids.client_a, ids.fp_a2, ids.dev_a2, 0, 10);
    raise exception 'revoked till could still sync';
  exception when sqlstate '28000' then null;
  end;
  raise notice 'ok 8 revocation';
end $$;

-- 9. Mirror columns match the device schema contract ───────────────────────
create temp table t_contract as select :'schema'::jsonb as doc;
do $$
declare
  t record;
  expected text[];
  actual text[];
  server_only text[] := array['client_id', 'server_seq', 'origin_device_id', 'last_event_id', 'received_at'];
begin
  for t in select entity_type from sync_entities loop
    select array_agg(value order by value) into expected
      from jsonb_array_elements_text((select doc from t_contract)->'tables'->t.entity_type->'columns');
    select array_agg(name order by name) into actual
      from public._sync_columns(t.entity_type) where name <> all (server_only);
    assert expected = actual, format('%s: contract %s vs mirror %s', t.entity_type, expected, actual);
  end loop;
  raise notice 'ok 9 schema parity';
end $$;

-- 10. Server strategies match the contract ─────────────────────────────────
do $$
declare
  t record;
  doc jsonb := (select doc from t_contract);
begin
  for t in select key, value->>'sync' as strategy from jsonb_each(doc->'tables') loop
    if t.strategy is null then
      assert not exists (select 1 from sync_entities where entity_type = t.key), format('%s is local-only', t.key);
    else
      assert (select strategy from sync_entities where entity_type = t.key) = t.strategy,
        format('%s: contract %s', t.key, t.strategy);
    end if;
  end loop;
  raise notice 'ok 10 strategy parity';
end $$;
