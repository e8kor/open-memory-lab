#!/usr/bin/env bash
# poc-demo.sh — the whole PoC story, end to end, against a throwaway database.
#
# Boots ./target/release/oml-server-lead on a fresh `oml_poc` database and
# drives every ceremony through ./target/release/oml (the lab client speaks
# only the public HTTP API, ADR-0015): bootstrap claim → workarea → keys →
# submit → (recall withholds the held row) → review + approve → recall hit →
# the R2 reviewer-is-author refusal → supersede → verify-chain → doctor →
# graceful shutdown.
#
# Re-runnable: the database is dropped and recreated on every run; the
# keyring and credentials live in a per-run temp directory.
#
# Prerequisites: the release binaries (cargo build --release), a local
# PostgreSQL with role oml/oml_dev_only holding CREATEDB, and jq.

set -u -o pipefail
cd "$(dirname "$0")/.."

PGHOST=127.0.0.1
PGUSER=oml
export PGPASSWORD="${PGPASSWORD:-oml_dev_only}"
DB=oml_poc
PORT="${OML_DEMO_PORT:-8321}"
SERVER_BIN=./target/release/oml-server-lead
OML=./target/release/oml
SERVER_URL="http://127.0.0.1:${PORT}"

DEMO_DIR="$(mktemp -d /tmp/oml-poc-demo.XXXXXX)"
OUT="$DEMO_DIR/last.json"
export OML_CLI_CREDENTIALS="$DEMO_DIR/credentials"
SERVER_PID=""

fail() {
    echo
    echo "DEMO FAILED: $*" >&2
    echo "--- server stderr tail ---" >&2
    tail -n 20 "$DEMO_DIR/server.stderr" 2>/dev/null >&2 || true
    exit 1
}

cleanup() {
    if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
        kill -TERM "$SERVER_PID" 2>/dev/null
        wait "$SERVER_PID" 2>/dev/null
    fi
}
trap cleanup EXIT

[ -x "$SERVER_BIN" ] && [ -x "$OML" ] || fail "release binaries missing — run: cargo build --release"
command -v jq >/dev/null || fail "jq is required"

# step <title> <expected-exit> <cmd…> — echoes the command, shows the
# response, saves stdout to $OUT, and enforces the expected exit code.
step() {
    local title="$1" want="$2"
    shift 2
    echo
    echo "=== $title"
    echo "\$ $*"
    "$@" | tee "$OUT"
    local code=${PIPESTATUS[0]}
    echo "(exit $code)"
    [ "$code" -eq "$want" ] || fail "'$title' exited $code, expected $want"
}

jqf() { jq -r "$1" "$OUT"; }

# --- a fresh database and a cold boot --------------------------------------
echo "=== fresh database $DB"
if (exec 3<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; then
    fail "port $PORT is already in use (a previous server still running?)"
fi
psql -h "$PGHOST" -U "$PGUSER" -d oml_dev -v ON_ERROR_STOP=1 \
    -c "DROP DATABASE IF EXISTS $DB WITH (FORCE)" \
    -c "CREATE DATABASE $DB" || fail "database reset (is PostgreSQL up?)"

echo
echo "=== booting oml-server-lead (migrate on boot, keyring minted in $DEMO_DIR)"
OML_POSTGRES__URL="postgres://$PGUSER:$PGPASSWORD@$PGHOST:5432/$DB" \
OML_SERVER__LISTEN="127.0.0.1:$PORT" \
OML_KMS__KEYRING_PATH="$DEMO_DIR/keyring" \
    "$SERVER_BIN" >"$DEMO_DIR/server.stdout" 2>"$DEMO_DIR/server.stderr" &
SERVER_PID=$!

# The one-time enrollment token is the show-once line on stdout; the raw
# signing public key is boot-logged for the out-of-band verifier handoff.
TOKEN=""
for _ in $(seq 1 100); do
    TOKEN="$(grep -o 'enr_[0-9a-f]\{48\}' "$DEMO_DIR/server.stdout" 2>/dev/null | head -1 || true)"
    [ -n "$TOKEN" ] && break
    kill -0 "$SERVER_PID" 2>/dev/null || fail "server exited during boot"
    sleep 0.1
done
[ -n "$TOKEN" ] || fail "no enrollment token appeared on the server's stdout"
READY=""
for _ in $(seq 1 100); do
    if curl -fsS "$SERVER_URL/healthz" >/dev/null 2>&1; then READY=1; break; fi
    kill -0 "$SERVER_PID" 2>/dev/null || fail "server exited before listening"
    sleep 0.1
done
[ -n "$READY" ] || fail "server never answered /healthz on $SERVER_URL"
# tracing's fmt layer colors field separators, so strip ANSI before grepping.
PUBKEY="$(sed 's/\x1b\[[0-9;]*m//g' "$DEMO_DIR/server.stderr" | grep -o 'public_key=[0-9a-f]\{64\}' | head -1 | cut -d= -f2)"
[ -n "$PUBKEY" ] || fail "no public_key in the server boot log"
echo "server up (pid $SERVER_PID); enrollment token captured: ${TOKEN:0:12}…"
echo "deployment signing public key (from the boot log, handed out of band): $PUBKEY"

# --- bootstrap: the one-time claim ------------------------------------------
step "bootstrap claim (single-use token -> first admin key, credentials stored 0600)" 0 \
    "$OML" --server "$SERVER_URL" --json bootstrap claim \
    --token "$TOKEN" --tenant-name poc-demo --principal platform/alice
TENANT="$(jqf .tenant_id)"
ROOT_WA="$(jqf .root_workarea_id)"

step "a second claim is refused — the token is consumed (typed refusal, exit 1)" 1 \
    "$OML" --server "$SERVER_URL" --json bootstrap claim --token "$TOKEN"

# --- workarea and the crew's keys (admin credentials from the stored file) ---
step "create a child workarea under the root ($ROOT_WA)" 0 \
    "$OML" --json workareas create "$ROOT_WA" poc-metals
WA="$(jqf .id)"

# Each key carries only grant roles its key role admits (the admissibility
# table, ADR-0025 d1): writer admits reader and writer, dreamer admits reader
# and dreamer, reader admits reader.
step "issue the agent's writer key with a writer and a reader grant (one key writes and recalls)" 0 \
    "$OML" --json keys issue --role writer --held-by agent \
    --principal crew/agent-blue --default-workarea "$WA" --grant "$WA:writer" --grant "$WA:reader"
WRITER="$(jqf .value)"

step "issue a dreamer key (held by a human reviewer)" 0 \
    "$OML" --json keys issue --role dreamer --held-by human \
    --principal crew/maya --default-workarea "$WA" --grant "$WA:dreamer"
DREAMER="$(jqf .value)"

step "issue a reader key for a second identity that only reads (a reader key admits reader grants only)" 0 \
    "$OML" --json keys issue --role reader --held-by service \
    --principal crew/recall-bot --default-workarea "$WA" --grant "$WA:reader"
READER="$(jqf .value)"

# --- submit: held on arrival, invisible to recall ----------------------------
step "submit a claim with the writer key (deterministic scan; held on arrival)" 0 \
    "$OML" --key "$WRITER" --json submit \
    --claim "Supplier Accra Metals quoted 412 USD/t for Q1-2027 delivery." \
    --claim-key supplier-quote-am-7741
ARTIFACT="$(jqf .artifact_id)"
[ "$(jqf .state)" = "held" ] || fail "submitted claim is not held"

step "recall while held: the claim is NOT served (held rows never leave the gate)" 0 \
    "$OML" --key "$READER" --json recall --query "Accra Metals"
[ "$(jqf '.hits | length')" = "0" ] || fail "a held row leaked into recall"

# --- review: serve, approve, and only then recall hits -----------------------
step "review next with the dreamer key (claims the one held item)" 0 \
    "$OML" --key "$DREAMER" --json review next
[ "$(jqf .item.artifact_id)" = "$ARTIFACT" ] || fail "review served a different artifact"

step "approve with the dreamer key (k=1 ratifies; the row promotes)" 0 \
    "$OML" --key "$DREAMER" --json review verdict --memory "$ARTIFACT" --approve
[ "$(jqf .state)" = "promoted" ] || fail "approval did not promote"

step "recall after promotion: the claim is served" 0 \
    "$OML" --key "$READER" --json recall --query "Accra Metals"
[ "$(jqf '.hits | length')" -ge 1 ] || fail "promoted row missing from recall"
jqf '.hits[0].memory.artifact_id' | grep -q "$ARTIFACT" || fail "recall hit is not the promoted artifact"

step "recall after promotion with the agent's own writer key (its reader grant serves recall)" 0 \
    "$OML" --key "$WRITER" --json recall --query "Accra Metals"
jqf '.hits[].memory.artifact_id' | grep -q "$ARTIFACT" || fail "the writer key's reader grant did not serve recall"

# --- R2: the reviewer is never the author ------------------------------------
step "issue a dreamer key to the AUTHOR's principal (crew/agent-blue also reviews?)" 0 \
    "$OML" --json keys issue --role dreamer --held-by agent \
    --principal crew/agent-blue --default-workarea "$WA" --grant "$WA:dreamer"
AUTHOR_DREAMER="$(jqf .value)"

step "verdict with the author's key is refused — R2 against the lineage closure (exit 1)" 1 \
    "$OML" --key "$AUTHOR_DREAMER" --json review verdict --memory "$ARTIFACT" --reject --reason "self-review attempt"
[ "$(jqf .error.code)" = "review.author_in_lineage" ] || fail "expected review.author_in_lineage, got $(jqf .error.code)"

# --- supersede: the author replaces the promoted tip -------------------------
step "supersede the promoted tip with the writer key (successor enters held)" 0 \
    "$OML" --key "$WRITER" --json supersede --target "$ARTIFACT" \
    --claim "Supplier Accra Metals revised the Q1-2027 quote to 399 USD/t."
SUCCESSOR="$(jqf .artifact_id)"
[ "$(jqf .state)" = "held" ] || fail "successor is not held"
[ "$(jqf .replaces.artifact_id)" = "$ARTIFACT" ] || fail "the successor does not name the version it replaces"

step "recall before ratification still serves the old tip (the tip moves only at ratification)" 0 \
    "$OML" --key "$READER" --json recall --query "Accra Metals"
jq -r '.hits[].memory.body' "$OUT" | grep -q "412 USD/t" || fail "the old tip left recall before ratification"
jq -r '.hits[].memory.body' "$OUT" | grep -q "399 USD/t" && fail "the held successor reached recall unreviewed"

step "the successor rides the same review gate: dreamer approves it" 0 \
    "$OML" --key "$DREAMER" --json review verdict --memory "$SUCCESSOR" --approve
[ "$(jqf .state)" = "promoted" ] || fail "successor did not promote"

step "recall now serves the ratified successor (the replaced version is history)" 0 \
    "$OML" --key "$READER" --json recall --query "Accra Metals"
jq -r '.hits[].memory.body' "$OUT" | grep -q "399 USD/t" || fail "successor body missing from recall"
jq -r '.hits[].memory.body' "$OUT" | grep -q "412 USD/t" && fail "the replaced version is still a recall hit"

# --- evidence: the chain export is the one verification surface -------------
# The events tail is a filtered feed (ADR-0025 d15), so live verification
# reads the chain export, which this build does not serve: the verb refuses
# by name (exit 2) rather than report breaks it did not find.
echo
echo "=== audit verify-chain --genesis: refused by name while no chain export is served"
"$OML" --key "$READER" audit verify-chain --genesis --signing-key "$PUBKEY" 2>"$DEMO_DIR/verify.stderr"
code=$?
cat "$DEMO_DIR/verify.stderr"
[ "$code" -eq 2 ] || fail "verify-chain exited $code, expected the local refusal 2"
grep -q "chain export" "$DEMO_DIR/verify.stderr" || fail "verify-chain did not name the chain export"

step "oml doctor" 0 \
    "$OML" --key "$READER" doctor

# --- clean shutdown ----------------------------------------------------------
echo
echo "=== clean shutdown (SIGTERM)"
kill -TERM "$SERVER_PID"
wait "$SERVER_PID" 2>/dev/null
SERVER_PID=""
grep -q "graceful shutdown complete" "$DEMO_DIR/server.stderr" || fail "no graceful shutdown in the server log"
echo "server stopped gracefully."
echo
echo "DEMO PASSED — database $DB left in place for inspection; run artifacts in $DEMO_DIR"
