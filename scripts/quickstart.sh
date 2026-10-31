#!/usr/bin/env bash
# Quickstart: turn a freshly booted oml-server into a usable crew.
#
# Claims the single-use enrollment token, creates one workarea, and issues two
# keys on it, naming the root `universe` and the workarea by its name wherever
# an id would go (ADR-0026 d4, d6): the agent's one key, a writer key with a
# writer and a reader grant, and a dreamer key (the reviewer, a different
# identity). Then checks
# the agent plugin end to end over /mcp on the agent's one key: remember ->
# held in its own work -> its held row in its own recall -> approved by the
# dreamer -> recalled (docs/api/mcp-plugin.md, "Wiring it into your agent";
# ADR-0025). Prints the environment the `oml` Claude Code plugin reads.
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

boot="$(oml bootstrap claim quickstart-admin --token "$TOKEN" --tenant-name quickstart --principal platform/admin)" \
    || fail "bootstrap claim refused (already claimed? the token is single-use): $boot"
workarea="$(oml workareas create universe "$WORKAREA_NAME" | jq -r .id)"

issue() { # key_name role held_by principal grant_role...
    local key_name="$1" role="$2" held_by="$3" principal="$4" grant issued
    local grants=()
    shift 4
    for grant in "$@"; do grants+=(--grant "$WORKAREA_NAME:$grant"); done
    issued="$(oml keys issue "$key_name" --role "$role" --held-by "$held_by" --principal "$principal" \
        --default-workarea "$WORKAREA_NAME" "${grants[@]}")"
    [ "$(jq -r .default_workarea_id <<<"$issued")" = "$workarea" ] \
        || fail "the workarea name $WORKAREA_NAME did not resolve to its id $workarea: $issued"
    jq -r .value <<<"$issued"
}
agent="$(issue quickstart-agent writer agent crew/agent writer reader)"
dreamer="$(issue quickstart-reviewer dreamer human crew/reviewer dreamer)"

rpc() { # id method params
    curl -fsS "$OML_URL/mcp" \
        -H "authorization: Bearer $agent" -H 'content-type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":$1,\"method\":\"$2\",\"params\":$3}"
}
call() { # id tool arguments
    rpc "$1" tools/call "{\"name\":\"$2\",\"arguments\":$3}"
}
state_of() { # list artifact_id response: the artifact's state in that list, if listed
    jq -r --arg id "$2" ".result.structuredContent.$1[]? | select(.artifact_id == \$id) | .state" <<<"$3"
}

tools="$(rpc 1 tools/list '{}' | jq -r '[.result.tools[].name] | join(", ")')"
held="$(call 2 oml_remember '{"claim_key":"quickstart-check","text":"The quickstart wrote this memory through the agent plugin.","idempotency_key":"quickstart/1"}')"
[ "$(jq -r .result.structuredContent.state <<<"$held")" = "held" ] || fail "oml_remember did not hold the write: $held"
artifact="$(jq -r .result.structuredContent.artifact_id <<<"$held")"

work="$(call 3 oml_my_work '{"claim_key":"quickstart-check"}')"
[ "$(state_of items "$artifact" "$work")" = "held" ] || fail "oml_my_work does not list the write as held: $work"
own="$(call 4 oml_recall '{"query":"quickstart wrote","include_own_held":true}')"
[ "$(state_of hits "$artifact" "$own")" = "held" ] || fail "oml_recall with include_own_held did not return the held write to its author: $own"

oml --key "$dreamer" review next >/dev/null
oml --key "$dreamer" review verdict --memory "$artifact" --approve >/dev/null
recalled="$(call 5 oml_recall '{"query":"quickstart wrote"}')"
[ "$(state_of hits "$artifact" "$recalled")" = "promoted" ] || fail "the approved memory did not come back from oml_recall: $recalled"

cat <<EOF
OML is ready at $OML_URL. The MCP check passed on the agent's one key: remember ->
held in its own work and its own recall -> approved by a different identity ->
recalled. Tools served: $tools.

Workarea "$WORKAREA_NAME": $workarea

Each key is shown once; store them now.
  agent   (writes and recalls)  $agent
  dreamer (a person reviews)    $dreamer

Give the agent its one key and install the plugin:
  export OML_URL=$OML_URL
  export OML_API_KEY=$agent
  claude plugin marketplace add e8kor/open-memory-lab
  claude plugin install oml@open-memory-lab

Nothing the agent writes reaches another key until the dreamer key approves it:
  ${CLI[*]} --server $CLI_SERVER --key <dreamer> review next
  ${CLI[*]} --server $CLI_SERVER --key <dreamer> review verdict --memory <artifact_id> --approve
EOF
