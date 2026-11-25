#!/usr/bin/env bash
# The read-only production counts before the v0.8.6 upgrade (ADR-0097 d27
# item 1). The operator runs it on the database host, never a lane:
#
#   PGPASSFILE=~/.pgpass \
#   OML_COUNTS_DATABASE_URL=postgres://<owner role>@<host>/<db> \
#     scripts/ops/read_only_counts.sh > counts.csv
#
# The URL names the database owner role (OML_POSTGRES__OWNER_URL's role),
# with no password: the password comes from PGPASSFILE or ~/.pgpass, never
# from the URL, so no process listing shows it. A URL that carries one is
# refused. The owner role is NOSUPERUSER NOBYPASSRLS and the tables FORCE
# row-level security, so the SQL lists the tenants under oml.startup_check
# and counts each one under its oml.tenant_id (read_only_counts.sql says
# how). The script changes nothing: the session is read-only by default
# (default_transaction_read_only), and the SQL runs in one READ ONLY
# transaction and ends in ROLLBACK. It prints CSV, `count,tenant,value`, one
# row per tenant and count:
#   unlinked_admin_keys, service_admin_keys, cross_root_grants,
#   admin_minted_user_keys, admin_minted_connect_codes, open_investigations,
#   admin_persons_ever (2 or more: the tenant had two admin persons).
# It exits non-zero, printing nothing on stdout, when the role sees no
# tenant: a count of nothing would read as a clean tenant.
# The HTTP API serves no tenant-wide key or grant listing to an admin key,
# so these counts read the database, not the API.
set -euo pipefail
: "${OML_COUNTS_DATABASE_URL:?set OML_COUNTS_DATABASE_URL to the database owner URL}"
case "$OML_COUNTS_DATABASE_URL" in
  *://*:*@*)
    echo "read_only_counts: the URL carries a password; put it in PGPASSFILE or ~/.pgpass instead" >&2
    exit 2
    ;;
esac
here="$(cd "$(dirname "$0")" && pwd)"
out="$(PGOPTIONS='-c default_transaction_read_only=on' psql \
  --no-psqlrc --quiet --tuples-only --no-align --field-separator=, \
  --set ON_ERROR_STOP=1 \
  --dbname "$OML_COUNTS_DATABASE_URL" \
  --file "$here/read_only_counts.sql")"
rows="$(printf '%s\n' "$out" | sed '1d' | grep -c . || true)"
if [ "$rows" -eq 0 ]; then
  echo "read_only_counts: no tenant is visible to this role; check that the URL names the database owner role of the right database" >&2
  exit 1
fi
printf '%s\n' "$out"
