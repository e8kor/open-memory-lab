#!/usr/bin/env bash
# Boots a lead built from this tree against a throwaway database, loads the
# recall eval, runs it through oml_read once per served pipeline, and gates
# the results (ADR-0049 d9). CI's recall-eval job runs it; so can anyone with
# a PostgreSQL superuser:
#
#   PGHOST=127.0.0.1 PGPORT=5432 PGUSER=oml PGPASSWORD=... \
#       eval/recall/served.sh <out-dir>
#
# OML_BIN_DIR names the directory holding oml-server-lead and oml (default
# target/release). EVAL_PIPELINES lists the pipelines measured (default all
# four, keyword-all included for comparison); EVAL_DEFAULT is the pipeline
# that must pass its gate (default the lead's own default, bm25-postings).
# The database and its role pair are dropped on exit; key values go to
# owner-only files in the out directory and are never printed.
set -u -o pipefail
cd "$(dirname "$0")/../.."

OUT="${1:?usage: served.sh <out-dir>}"
mkdir -p "$OUT"
BIN="${OML_BIN_DIR:-target/release}"
PIPELINES="${EVAL_PIPELINES:-keyword-all keyword-any bm25 bm25-trigram bm25-postings}"
DEFAULT="${EVAL_DEFAULT:-bm25-postings}"
PGHOST="${PGHOST:-127.0.0.1}"
# No default port: a guessed one could reach a live server.
PGPORT="${PGPORT:?set PGPORT to a throwaway PostgreSQL cluster}"
PGUSER="${PGUSER:-oml}"
export PGPASSWORD="${PGPASSWORD:-oml_dev_only}"
PORT="${EVAL_PORT:-8437}"
URL="http://127.0.0.1:$PORT"
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
    echo "recall eval: $*" >&2
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

ENABLED="$(echo "$PIPELINES" | tr ' ' ',')"
OML_POSTGRES__URL="postgres://${DB}_serving:$PAIR_PASSWORD@$PGHOST:$PGPORT/$DB" \
OML_POSTGRES__OWNER_URL="postgres://${DB}_owner:$PAIR_PASSWORD@$PGHOST:$PGPORT/$DB" \
OML_SERVER__LISTEN="127.0.0.1:$PORT" \
OML_KMS__KEYRING_PATH="$STATE/keyring" \
OML_SEARCH__ENABLED="$ENABLED" \
OML_SEARCH__DEFAULT="$DEFAULT" \
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
    --tenant-name eval --principal eval/admin <<<"$TOKEN" >/dev/null || fail "bootstrap claim"
"$OML" --json workareas create universe eval-crew >/dev/null || fail "workarea"
(umask 077
 "$OML" --json keys issue eval-writer --role writer --held-by agent --principal crew/writer \
     --default-workarea eval-crew --grant eval-crew:writer --grant eval-crew:reader \
     | jq -r .value >"$STATE/writer.key"
 "$OML" --json keys issue eval-reviewer --role dreamer --held-by human --principal crew/reviewer \
     --default-workarea eval-crew --grant eval-crew:dreamer \
     | jq -r .value >"$STATE/reviewer.key") || fail "keys"

for pipeline in $PIPELINES; do
    python3 eval/recall/run.py --url "$URL" --writer-key-file "$STATE/writer.key" \
        --reviewer-key-file "$STATE/reviewer.key" --out "$OUT" --search "$pipeline" \
        || fail "run $pipeline"
done
# The plugin's failure hook sends a few distinctive words, k 3 under 200
# tokens (ADR-0038 d6): measured on the default, as the hook now runs it, and
# on keyword-all, as it ran before (ADR-0049 d10).
for pipeline in $(printf '%s\n' "$DEFAULT" keyword-all | sort -u); do
    case " $PIPELINES " in *" $pipeline "*) ;; *) continue ;; esac
    python3 eval/recall/run.py --url "$URL" --writer-key-file "$STATE/writer.key" \
        --reviewer-key-file "$STATE/reviewer.key" --out "$OUT" --search "$pipeline" \
        --keywords --k 3 --max-tokens 200 \
        --name "hook_$(echo "$pipeline" | tr '-' '_')" || fail "hook run $pipeline"
done
rm -f "$OUT/idmap.json"

python3 eval/recall/gate.py --results "$OUT" --default "$DEFAULT"
