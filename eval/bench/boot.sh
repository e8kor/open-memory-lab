#!/usr/bin/env bash
# Boots a lead built from this tree against a throwaway database for the
# benchmark harness, provisions one workarea per benchmark under a parent
# `bench`, issues the two keys the harness needs, and stays up until it is
# stopped (SIGINT or SIGTERM), when it drops the database and its role pair.
#
#   PGHOST=127.0.0.1 PGPORT=55440 PGUSER=postgres \
#       eval/bench/boot.sh <state-dir> <workarea>...
#
# The workareas are the names bench.py derives (bench-<dataset>, and for a
# dataset of several haystacks bench-<dataset>-<group>); `bench.py
# workareas <dataset>...` prints them. Keys: <state-dir>/writer.key, an agent
# key with writer and reader grants on the `bench` subtree, and
# <state-dir>/reviewer-<i>.key for i in 1..BENCH_REVIEWERS (default 4), human
# keys of other principals with the dreamer grant, so the reviewer is never
# the author (R2). Several reviewers because review next serves one key its
# own claimed row until it is judged, so one key reviews one row at a time;
# a crew of reviewers judges in parallel, as one would. Values go to owner-only
# files and are never printed. <state-dir>/url holds the server's URL once it
# is ready; <state-dir>/server.stderr its log.
#
# The same boot as eval/recall/served.sh, kept separate because that script
# runs one fixed set and exits, and this one serves many runs.
set -u -o pipefail
cd "$(dirname "$0")/../.."

STATE="${1:?usage: boot.sh <state-dir> <workarea>...}"
shift
[ "$#" -ge 1 ] || { echo "boot.sh: name at least one workarea" >&2; exit 2; }
mkdir -p "$STATE"
chmod 700 "$STATE"
BIN="${OML_BIN_DIR:-target/release}"
PIPELINES="${BENCH_PIPELINES:-keyword-all keyword-any bm25 bm25-trigram bm25-postings}"
DEFAULT="${BENCH_DEFAULT:-bm25-postings}"
PGHOST="${PGHOST:-127.0.0.1}"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-postgres}"
PORT="${BENCH_PORT:-8447}"
REVIEWERS="${BENCH_REVIEWERS:-4}"
URL="http://127.0.0.1:$PORT"
# The metrics listener: the sparse encoder's backlog is read there
# (ADR-0052 d15; wait_sparse.sh).
METRICS_PORT="${BENCH_METRICS_PORT:-9447}"
TAG="$(od -An -N4 -tx1 /dev/urandom | tr -d ' \n')"
DB="oml_bench_$TAG"
PAIR_PASSWORD="$(od -An -N12 -tx1 /dev/urandom | tr -d ' \n')"
export OML_CLI_CREDENTIALS="$STATE/credentials"
unset OML_URL OML_API_KEY
SERVER_PID=""
MINTED=""

admin() { psql -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d postgres -v ON_ERROR_STOP=1 -qtA "$@"; }
fail() {
    echo "bench boot: $*" >&2
    tail -n 30 "$STATE/server.stderr" 2>/dev/null >&2 || true
    exit 1
}
cleanup() {
    rm -f "$STATE/url"
    if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
        kill -TERM "$SERVER_PID" 2>/dev/null
        wait "$SERVER_PID" 2>/dev/null
    fi
    if [ -n "$MINTED" ] && [ -z "${BENCH_KEEP_DB:-}" ]; then
        admin -c "DROP DATABASE IF EXISTS $DB WITH (FORCE)" \
            -c "DROP ROLE IF EXISTS ${DB}_owner" -c "DROP ROLE IF EXISTS ${DB}_serving" || true
    fi
    rm -f "$STATE"/writer.key "$STATE"/reviewer-*.key "$STATE/credentials" "$STATE/keyring"
}
trap cleanup EXIT
trap 'exit 0' INT TERM

[ -x "$BIN/oml-server-lead" ] && [ -x "$BIN/oml" ] || fail "binaries missing in $BIN"
command -v jq >/dev/null || fail "jq is required"

admin -c "CREATE ROLE ${DB}_owner LOGIN PASSWORD '$PAIR_PASSWORD'" \
    -c "CREATE ROLE ${DB}_serving LOGIN PASSWORD '$PAIR_PASSWORD'" || fail "role pair"
MINTED=1
admin -c "CREATE DATABASE $DB OWNER ${DB}_owner" || fail "database"
echo "$DB" >"$STATE/database"

ENABLED="$(echo "$PIPELINES" | tr ' ' ',')"
OML_POSTGRES__URL="postgres://${DB}_serving:$PAIR_PASSWORD@$PGHOST:$PGPORT/$DB" \
OML_POSTGRES__OWNER_URL="postgres://${DB}_owner:$PAIR_PASSWORD@$PGHOST:$PGPORT/$DB" \
OML_SERVER__LISTEN="127.0.0.1:$PORT" \
OML_SERVER__METRICS_LISTEN="127.0.0.1:$METRICS_PORT" \
OML_KMS__KEYRING_PATH="$STATE/keyring" \
OML_SEARCH__ENABLED="$ENABLED" \
OML_SEARCH__DEFAULT="$DEFAULT" \
    "$BIN/oml-server-lead" >"$STATE/server.stdout" 2>"$STATE/server.stderr" &
SERVER_PID=$!

TOKEN=""
for _ in $(seq 1 300); do
    TOKEN="$(grep -o 'enr_[0-9a-f]\{48\}' "$STATE/server.stdout" 2>/dev/null | head -1 || true)"
    [ -n "$TOKEN" ] && break
    kill -0 "$SERVER_PID" 2>/dev/null || fail "the lead exited during boot"
    sleep 0.1
done
[ -n "$TOKEN" ] || fail "no enrollment token"
for _ in $(seq 1 100); do
    curl -fsS "$URL/healthz" >/dev/null 2>&1 && break
    sleep 0.1
done

OML="$BIN/oml"
"$OML" --url "$URL" --json bootstrap claim bench-admin --token-stdin \
    --tenant-name bench --principal bench/admin <<<"$TOKEN" >/dev/null || fail "bootstrap claim"
"$OML" --json workareas create universe bench >/dev/null || fail "workarea bench"
for wa in "$@"; do
    "$OML" --json workareas create bench "$wa" >/dev/null || fail "workarea $wa"
done
(umask 077
 "$OML" --json keys issue bench-writer --role writer --held-by agent --principal bench/writer \
     --default-workarea bench --grant bench:writer --grant bench:reader \
     | jq -r .value >"$STATE/writer.key"
 for i in $(seq 1 "$REVIEWERS"); do
     "$OML" --json keys issue "bench-reviewer-$i" --role dreamer --held-by human \
         --principal "bench/reviewer-$i" --default-workarea bench --grant bench:dreamer \
         | jq -r .value >"$STATE/reviewer-$i.key" || exit 1
 done) || fail "keys"
[ -s "$STATE/writer.key" ] && [ -s "$STATE/reviewer-1.key" ] || fail "empty key file"

echo "http://127.0.0.1:$METRICS_PORT" >"$STATE/metrics_url"
echo "$URL" >"$STATE/url"
echo "bench boot: serving $URL, database $DB, workareas $*" >&2
wait "$SERVER_PID"
fail "the lead exited"
