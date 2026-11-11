#!/usr/bin/env bash
# The federated recall eval (ADR-0072 d11): boots a lead built from this tree
# against a throwaway database, makes one committed workarea per corpus topic
# under eval-fed, turns federation on, and runs federated.py: scope, wide and
# the union on the scoped and cross splits, the widen hint, and, given
# SciFact's source.zip, the skew split.
#
#   PGHOST=127.0.0.1 PGPORT=5432 PGUSER=oml PGPASSWORD=... \
#       eval/recall/federated.sh <out-dir> [path/to/scifact/source.zip]
#
# OML_BIN_DIR names the directory holding oml-server-lead and oml (default
# target/release). The database and its role pair are dropped on exit; key
# values go to owner-only files in a state directory and are never printed.
set -u -o pipefail
cd "$(dirname "$0")/../.."

OUT="${1:?usage: federated.sh <out-dir> [scifact source.zip]}"
SCIFACT="${2:-}"
mkdir -p "$OUT"
BIN="${OML_BIN_DIR:-target/release}"
PGHOST="${PGHOST:-127.0.0.1}"
# No default port: a guessed one could reach a live server.
PGPORT="${PGPORT:?set PGPORT to a throwaway PostgreSQL cluster}"
PGUSER="${PGUSER:-oml}"
export PGPASSWORD="${PGPASSWORD:-oml_dev_only}"
PORT="${EVAL_PORT:-8437}"
URL="http://127.0.0.1:$PORT"
# The metrics listener: the sparse encoder's backlog is read there
# (ADR-0052 d15; wait_sparse.sh).
METRICS_PORT="${EVAL_METRICS_PORT:-9437}"
TAG="$(od -An -N4 -tx1 /dev/urandom | tr -d ' \n')"
DB="oml_eval_$TAG"
PAIR_PASSWORD="$(od -An -N12 -tx1 /dev/urandom | tr -d ' \n')"
STATE="$(mktemp -d "$OUT/state.XXXXXX")"
chmod 700 "$STATE"
export OML_CLI_CREDENTIALS="$STATE/credentials"
unset OML_URL OML_API_KEY
SERVER_PID=""
MINTED=""

admin() { psql -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d postgres -v ON_ERROR_STOP=1 -qtA "$@"; }
fail() {
    echo "federated eval: $*" >&2
    tail -n 30 "$STATE/server.stderr" 2>/dev/null >&2 || true
    exit 1
}
cleanup() {
    if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
        kill -TERM "$SERVER_PID" 2>/dev/null
        wait "$SERVER_PID" 2>/dev/null
    fi
    if [ -n "$MINTED" ]; then
        admin -c "DROP DATABASE IF EXISTS $DB WITH (FORCE)" \
            -c "DROP ROLE IF EXISTS ${DB}_owner" -c "DROP ROLE IF EXISTS ${DB}_serving" || true
    fi
    rm -rf "$STATE"
}
trap cleanup EXIT

[ -x "$BIN/oml-server-lead" ] && [ -x "$BIN/oml" ] || fail "binaries missing in $BIN"
command -v jq >/dev/null || fail "jq is required"

admin -c "CREATE ROLE ${DB}_owner LOGIN PASSWORD '$PAIR_PASSWORD'" \
    -c "CREATE ROLE ${DB}_serving LOGIN PASSWORD '$PAIR_PASSWORD'" || fail "role pair"
MINTED=1
admin -c "CREATE DATABASE $DB OWNER ${DB}_owner" || fail "database"

OML_POSTGRES__URL="postgres://${DB}_serving:$PAIR_PASSWORD@$PGHOST:$PGPORT/$DB" \
OML_POSTGRES__OWNER_URL="postgres://${DB}_owner:$PAIR_PASSWORD@$PGHOST:$PGPORT/$DB" \
OML_SERVER__LISTEN="127.0.0.1:$PORT" \
OML_SERVER__METRICS_LISTEN="127.0.0.1:$METRICS_PORT" \
OML_KMS__KEYRING_PATH="$STATE/keyring" \
OML_SEARCH__ENABLED=bm25,bm25-postings \
OML_SEARCH__DEFAULT=bm25-postings \
    "$BIN/oml-server-lead" >"$STATE/server.stdout" 2>"$STATE/server.stderr" &
SERVER_PID=$!

TOKEN=""
for _ in $(seq 1 200); do
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
"$OML" --url "$URL" --json bootstrap claim eval-admin --token-stdin \
    --tenant-name evalfed --principal eval/admin <<<"$TOKEN" >/dev/null || fail "bootstrap claim"
"$OML" --json workareas create universe eval-fed >/dev/null || fail "workarea"
TOPICS="$(python3 -c 'import json; print(" ".join(sorted({m["topic"] for m in json.load(open("eval/recall/corpus.json"))["memories"]})))')"
for topic in $TOPICS; do
    "$OML" --json workareas create eval-fed "fed-$topic" >/dev/null || fail "workarea fed-$topic"
done
(umask 077
 "$OML" --json keys issue fed-writer --role writer --held-by agent --principal crew/writer \
     --default-workarea eval-fed --grant eval-fed:writer --grant eval-fed:reader \
     | jq -r .value >"$STATE/writer.key"
 "$OML" --json keys issue fed-reviewer --role dreamer --held-by human --principal crew/reviewer \
     --default-workarea eval-fed --grant eval-fed:dreamer --grant eval-fed:reader \
     | jq -r .value >"$STATE/reviewer.key"
 "$OML" --json keys issue fed-reader --role reader --held-by agent --principal crew/reader \
     --default-workarea eval-fed --grant eval-fed:reader \
     | jq -r .value >"$STATE/reader.key") || fail "keys"
# Each topic is committed: the admin proposes, the person ratifies (ADR-0009 d3).
for topic in $TOPICS; do
    id="$("$OML" --json workareas status propose "fed-$topic" --to committed | jq -r .transition_id)" \
        || fail "propose fed-$topic"
    "$OML" --url "$URL" --api-key "$(cat "$STATE/reviewer.key")" --json \
        workareas status ratify "fed-$topic" "$id" >/dev/null || fail "ratify fed-$topic"
done
"$OML" --json search federation --enabled on >/dev/null || fail "federation on"

OML="$OML" python3 eval/recall/federated.py --url "$URL" --state "$STATE" --out "$OUT" \
    ${SCIFACT:+--scifact-zip "$SCIFACT"} || fail "federated.py"
