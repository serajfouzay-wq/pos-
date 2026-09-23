-- Generator workspace, v1. Same conventions as the POS database: STRICT
-- tables, UUID text ids, fixed-format UTC timestamps, soft deletes only.
-- Not encrypted: it holds client settings and public material only. The
-- signing key stays in its own encrypted file and the GitHub token in the OS
-- credential store.

CREATE TABLE clients (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  slug TEXT NOT NULL,
  display_name TEXT NOT NULL,
  config TEXT NOT NULL CHECK (json_valid(config)),
  notes TEXT NOT NULL DEFAULT ''
) STRICT;
CREATE UNIQUE INDEX clients_slug ON clients (slug) WHERE deleted_at IS NULL;

-- Uploaded images. Replacing one soft-deletes the previous row.
CREATE TABLE client_assets (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  client_id TEXT NOT NULL REFERENCES clients (id),
  kind TEXT NOT NULL CHECK (kind IN ('receipt_logo', 'app_icon')),
  sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
  width INTEGER NOT NULL CHECK (width > 0),
  height INTEGER NOT NULL CHECK (height > 0),
  data BLOB NOT NULL
) STRICT;
CREATE UNIQUE INDEX client_assets_current ON client_assets (client_id, kind) WHERE deleted_at IS NULL;

-- Every license this generator signed (the token is public; it only works
-- on the machine it was issued for).
CREATE TABLE issued_licenses (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  client_id TEXT NOT NULL REFERENCES clients (id),
  device_name TEXT NOT NULL,
  fingerprint_hash TEXT NOT NULL CHECK (length(fingerprint_hash) = 64),
  max_devices INTEGER NOT NULL CHECK (max_devices > 0),
  issued_at TEXT NOT NULL,
  expires_at TEXT,
  token TEXT NOT NULL
) STRICT;
CREATE INDEX issued_licenses_client ON issued_licenses (client_id, issued_at);

CREATE TABLE builds (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  client_id TEXT NOT NULL REFERENCES clients (id),
  status TEXT NOT NULL CHECK (status IN ('publishing', 'queued', 'in_progress', 'succeeded', 'failed', 'cancelled', 'error')),
  config_sha256 TEXT NOT NULL CHECK (length(config_sha256) = 64),
  app_version TEXT NOT NULL,
  commit_sha TEXT,
  run_id INTEGER,
  run_url TEXT,
  artifact_id INTEGER,
  artifact_name TEXT,
  artifact_size INTEGER,
  download_path TEXT,
  message TEXT,
  completed_at TEXT
) STRICT;
CREATE INDEX builds_client ON builds (client_id, created_at);

CREATE TABLE settings (
  id TEXT PRIMARY KEY CHECK (length(id) = 36),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  key TEXT NOT NULL UNIQUE,
  value TEXT NOT NULL CHECK (json_valid(value))
) STRICT;

CREATE TRIGGER clients_no_delete BEFORE DELETE ON clients BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER client_assets_no_delete BEFORE DELETE ON client_assets BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER issued_licenses_no_delete BEFORE DELETE ON issued_licenses BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER builds_no_delete BEFORE DELETE ON builds BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER settings_no_delete BEFORE DELETE ON settings BEGIN SELECT RAISE(ABORT, 'hard deletes are forbidden; set deleted_at'); END;
CREATE TRIGGER issued_licenses_append_only BEFORE UPDATE ON issued_licenses BEGIN SELECT RAISE(ABORT, 'issued_licenses are append-only'); END;
