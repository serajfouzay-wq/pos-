-- Phase 9: Linux tills (AppImage) get updates from the channel too, and the
-- generator (not the build workflow) signs and publishes releases.
alter table public.app_releases drop constraint app_releases_target_check;
alter table public.app_releases add constraint app_releases_target_check
  check (target in ('windows-x86_64', 'linux-x86_64'));
comment on column public.app_releases.signature is
  'Minisign signature made by the generator with its update key (base64 of the signature file).';
