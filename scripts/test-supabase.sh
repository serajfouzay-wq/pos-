#!/usr/bin/env bash
# Runs the Supabase migrations and SQL tests against a throwaway database.
#   PGHOST/PGPORT/PGUSER select the server (default: local socket, postgres).
set -euo pipefail
cd "$(dirname "$0")/.."
DB="${POS_TEST_DB:-pos_sync_test}"
PSQL=(psql -v ON_ERROR_STOP=1 -q -X)
"${PSQL[@]}" -d postgres -c "drop database if exists ${DB}" -c "create database ${DB}"
"${PSQL[@]}" -d "$DB" -f supabase/tests/roles.sql
for migration in supabase/migrations/*.sql; do
  "${PSQL[@]}" -d "$DB" -f "$migration"
done
"${PSQL[@]}" -d "$DB" -v schema="$(cat packages/shared/contracts/db-schema.json)" \
  -f supabase/tests/sync.test.sql -o /dev/null
echo "supabase SQL tests passed"
