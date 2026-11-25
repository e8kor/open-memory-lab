#!/usr/bin/env bash
# The read-only production counts before the v0.8.6 upgrade (ADR-0097 d27
# item 1). The operator runs it on the database host, never a lane:
#
#   OML_COUNTS_DATABASE_URL=postgres://<owner role>@<host>/<db> \
#     scripts/ops/read_only_counts.sh > counts.csv
#
# The URL names the database owner role (OML_POSTGRES__OWNER_URL's role) or
# another role that reads every tenant's rows; the serving role reads one
# tenant at a time under row-level security and would count nothing. The
# script changes nothing: the session is read-only by default
# (default_transaction_read_only), the SQL runs in one READ ONLY transaction
# and ends in ROLLBACK,. It
# prints CSV, `count,tenant,value`, one row per tenant and count:
#   unlinked_admin_keys, service_admin_keys, cross_root_grants,
#   admin_minted_user_keys, admin_minted_connect_codes, open_investigations,
#   admin_persons_ever (2 or more: the tenant had two admin persons).
# The HTTP API serves no tenant-wide key or grant listing to an admin key,
# so these counts read the database, not the API.
set -euo pipefail
: "${OML_COUNTS_DATABASE_URL:?set OML_COUNTS_DATABASE_URL to the database owner URL}"
here="$(cd "$(dirname "$0")" && pwd)"
PGOPTIONS='-c default_transaction_read_only=on' exec psql \
  --no-psqlrc --quiet --tuples-only --no-align --field-separator=, \
  --set ON_ERROR_STOP=1 \
  --dbname "$OML_COUNTS_DATABASE_URL" \
  --file "$here/read_only_counts.sql"
