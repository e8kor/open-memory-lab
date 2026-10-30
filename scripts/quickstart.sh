#!/usr/bin/env bash
# Quickstart: turn a freshly booted oml-server into a usable crew.
#
# Claims the single-use enrollment token, creates one workarea, issues a
# writer key (the agent), a dreamer key (the reviewer, a different identity)
# and a reader key, then checks the agent plugin end to end over /mcp:
# remember -> held -> approve -> recall. Prints the environment the `oml`
# Claude Code plugin reads.
#
# Two ways to run it:
#   docker compose up -d && scripts/quickstart.sh
#       the CLI runs inside the oml-server container; the token is read from
#       its log.
#   OML_CLI=./target/release/oml OML_ENROLLMENT_TOKEN=enr_... scripts/quickstart.sh
#       against a server you started yourself.
#
# Environment:
#   OML_URL               server URL as seen from this shell (default http://127.0.0.1:8080)
#   OML_CLI               the oml command (default: docker compose exec -T oml-server oml)
#   OML_CLI_SERVER        server URL as seen by OML_CLI (default: 127.0.0.1:8080 inside
#                         the container, else OML_URL)
#   OML_ENROLLMENT_TOKEN  the token the server printed at first boot (default: read
#                         from `docker compose logs oml-server`)
#   OML_WORKAREA_NAME     the workarea to create (default: crew)

set -euo pipefail
cd "$(dirname "$0")/.."

OML_URL="${OML_URL:-http://127.0.0.1:8080}"
WORKAREA_NAME="${OML_WORKAREA_NAME:-crew}"
if [ -n "${OML_CLI:-}" ]; then
    read -r -a CLI <<<"$OML_CLI"
    CLI_SERVER="${OML_CLI_SERVER:-$OML_URL}"
    IN_COMPOSE=""
else
    CLI=(docker compose exec -T oml-server oml)
    CLI_SERVER="${OML_CLI_SERVER:-http://127.0.0.1:8080}"
    IN_COMPOSE=1
fi

fail() {
    echo "quickstart: $*" >&2
    exit 1
}
for tool in curl jq; do
    command -v "$tool" >/dev/null || fail "$tool is required"
done
oml() { "${CLI[@]}" --server "$CLI_SERVER" --json "$@"; }

ready=""
for _ in $(seq 1 60); do
    if curl -fsS "$OML_URL/readyz" >/dev/null 2>&1; then
        ready=1
        break
    fi
    sleep 1
done
[ -n "$ready" ] || fail "no ready server at $OML_URL/readyz"

TOKEN="${OML_ENROLLMENT_TOKEN:-}"
if [ -z "$TOKEN" ] && [ -n "$IN_COMPOSE" ]; then
    TOKEN="$(docker compose logs oml-server 2>/dev/null | grep -o 'enr_[0-9a-f]\{48\}' | tail -1 || true)"
fi
[ -n "$TOKEN" ] || fail "no enrollment token: set OML_ENROLLMENT_TOKEN to the token the server printed at first boot"

boot="$(oml bootstrap claim --token "$TOKEN" --tenant-name quickstart --principal platform/admin)" \
    || fail "bootstrap claim refused (already claimed? the token is single-use): $boot"
root="$(jq -r .root_workarea_id <<<"$boot")"
workarea="$(oml workareas create "$root" "$WORKAREA_NAME" | jq -r .id)"

issue() { # role held_by principal
    oml keys issue --role "$1" --held-by "$2" --principal "$3" \
        --default-workarea "$workarea" --grant "$workarea:$1" | jq -r .value
}
writer="$(issue writer agent crew/agent)"
dreamer="$(issue dreamer human crew/reviewer)"
reader="$(issue reader agent crew/recall)"

rpc() { # key id method params
    curl -fsS "$OML_URL/mcp" \
        -H "authorization: Bearer $1" -H 'content-type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":$2,\"method\":\"$3\",\"params\":$4}"
}
tools="$(rpc "$writer" 1 tools/list '{}' | jq -r '[.result.tools[].name] | join(", ")')"
held="$(rpc "$writer" 2 tools/call '{"name":"oml_remember","arguments":{"claim_key":"quickstart-check","text":"The quickstart wrote this memory through the agent plugin.","idempotency_key":"quickstart/1"}}')"
[ "$(jq -r .result.structuredContent.state <<<"$held")" = "held" ] || fail "oml_remember did not hold the write: $held"
artifact="$(jq -r .result.structuredContent.artifact_id <<<"$held")"

oml --key "$dreamer" review next >/dev/null
oml --key "$dreamer" review verdict --memory "$artifact" --approve >/dev/null
hits="$(rpc "$reader" 3 tools/call '{"name":"oml_recall","arguments":{"query":"quickstart wrote"}}' |
    jq '.result.structuredContent.hits | length')"
[ "$hits" -ge 1 ] || fail "the approved memory did not come back from oml_recall"

cat <<EOF
OML is ready at $OML_URL. The MCP check passed: remember -> held -> approved by
a different identity -> recalled. Tools served: $tools.

Workarea "$WORKAREA_NAME": $workarea

Each key is shown once; store them now.
  writer  (the agent writes)   $writer
  reader  (the agent recalls)  $reader
  dreamer (a person reviews)   $dreamer

Give the agent the writer and reader keys and install the plugin:
  export OML_URL=$OML_URL
  export OML_API_KEY=$writer
  export OML_RECALL_KEY=$reader
  claude plugin marketplace add e8kor/open-memory-lab
  claude plugin install oml@open-memory-lab

Nothing the agent writes is recalled until the dreamer key approves it:
  ${CLI[*]} --server $CLI_SERVER --key <dreamer> review next
  ${CLI[*]} --server $CLI_SERVER --key <dreamer> review verdict --memory <artifact_id> --approve
EOF
