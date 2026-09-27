-- Phase 7: mirror of the tills' Z reports (see the local migration
-- 0004_reports.sql). Append-only like transactions and audit_log. The
-- headline columns let a cloud back office list each shop's closings
-- without parsing `report`, the snapshot that was printed.

insert into public.sync_entities (entity_type, strategy, derived_columns) values
  ('z_reports', 'append_only', '{}');

create table public.z_reports (
  id uuid primary key,
  created_at timestamptz not null,
  updated_at timestamptz not null,
  deleted_at timestamptz,
  device_id uuid not null,
  z_number bigint not null,
  period_start timestamptz not null,
  period_end timestamptz not null,
  run_by uuid not null,
  currency text not null,
  sale_count bigint not null,
  refund_count bigint not null,
  void_count bigint not null,
  gross_sales bigint not null,
  discount_total bigint not null,
  refund_total bigint not null,
  void_total bigint not null,
  net_sales bigint not null,
  tax_total bigint not null,
  grand_total bigint not null,
  report jsonb not null,
  client_id uuid not null,
  server_seq bigint not null,
  origin_device_id uuid,
  last_event_id uuid,
  received_at timestamptz not null
);
create index z_reports_client_seq on public.z_reports (client_id, server_seq);
create unique index z_reports_device_number on public.z_reports (client_id, device_id, z_number);
alter table public.z_reports enable row level security;
revoke all on public.z_reports from anon, authenticated;
create trigger z_reports_no_delete before delete on public.z_reports for each row execute function public.forbid_hard_delete();
create trigger z_reports_append_only before update on public.z_reports for each row execute function public.forbid_update();
