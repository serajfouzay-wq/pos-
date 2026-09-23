#!/usr/bin/env bash
# End-to-end sync: fresh Postgres database + the real edge functions (served
# by supabase/functions/dev-server.ts) + two POS tills syncing over HTTP.
#   PGHOST/PGPORT/PGUSER(/PGPASSWORD) select the Postgres server (it must also
#   accept TCP on 127.0.0.1:PGPORT, or set POS_E2E_DB_URL); DENO overrides `deno`.
set -euo pipefail
cd "$(dirname "$0")/.."
DB="${POS_E2E_DB:-pos_sync_e2e}"
PORT="${POS_E2E_PORT:-54321}"
DENO="${DENO:-deno}"
PSQL=(psql -v ON_ERROR_STOP=1 -q -X)

"${PSQL[@]}" -d postgres -c "drop database if exists ${DB}" -c "create database ${DB}"
"${PSQL[@]}" -d "$DB" -f supabase/tests/roles.sql
for migration in supabase/migrations/*.sql; do
  "${PSQL[@]}" -d "$DB" -f "$migration"
done

# The edge functions connect over TCP (postgres.js); override for other setups.
export SUPABASE_DB_URL="${POS_E2E_DB_URL:-postgres://${PGUSER:-postgres}${PGPASSWORD:+:$PGPASSWORD}@127.0.0.1:${PGPORT:-5432}/${DB}}"
export LICENSE_PUBLIC_KEY_PEM="$(cat keys/dev/license-dev.public.pem)"
export PORT
DENO_NO_PACKAGE_JSON=1 "$DENO" run --no-config -A supabase/functions/dev-server.ts &
server=$!
trap 'kill $server 2>/dev/null || true' EXIT
for _ in $(seq 1 50); do
  curl -s -o /dev/null "http://127.0.0.1:${PORT}/" && break
  sleep 0.2
done

POS_E2E_SUPABASE_URL="http://127.0.0.1:${PORT}" cargo test -p pos-client --lib \
  sync::tests::live -- --ignored --nocapture
echo "sync e2e passed"
