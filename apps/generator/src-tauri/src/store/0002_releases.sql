-- v2 (Phase 8): builds carry the client version they produce, the release
-- notes, and whether the installer is published to the tills as an update.
-- `app_version` now holds that client version (MAJOR.MINOR of the app, then
-- a per-client build number).
ALTER TABLE builds ADD COLUMN release_notes TEXT NOT NULL DEFAULT '';
ALTER TABLE builds ADD COLUMN publish_update INTEGER NOT NULL DEFAULT 0 CHECK (publish_update IN (0, 1));
