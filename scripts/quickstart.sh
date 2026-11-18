#!/usr/bin/env bash
# Quickstart: turn a freshly booted oml-server into a usable crew.
#
# Claims the single-use enrollment token, creates one workspace, and issues two
# keys on it, naming the root `universe` and the workspace by its name wherever
# an id would go (ADR-0026 d4, d6): the agent's one key, a writer key with a
# writer and a reader grant, and a reviewer key (the reviewer, a different
# identity). Then checks
# the agent plugin end to end over /mcp on the agent's one key, through
# oml_write and oml_read alone: write -> held in its own work -> its held row
# in its own recall -> approved by the reviewer, as its own work's state says ->
# recalled (docs/api/mcp-plugin.md, "Wiring it into your agent"; ADR-0025,
# ADR-0039 d2). Then an upload session of three bytes opens, takes its part,
# completes and is abandoned on the agent's key, which proves the server can
# write the directory storage.url names (ADR-0004 d3). Prints the environment
# the `oml` Claude Code plugin reads.
#
# Two ways to run it:
#   docker compose up -d && scripts/quickstart.sh
#       the CLI runs inside the oml-server container; the token is read from
#       its log.
#   OML_CLI=./target/release/oml OML_ENROLLMENT_TOKEN=enr_... scripts/quickstart.sh
#       against a server you started yourself; or, with the CLI from a release,
#       verified and extracted as docs/cli/reference.md "Installing oml" shows,
#       OML_CLI=./oml-cli-X.Y.Z-<target>/oml.
#
# Environment:
#   OML_URL               server URL as seen from this shell (default http://127.0.0.1:8080)
#   OML_CLI               the oml command (default: docker compose exec -T oml-server oml)
#   OML_CLI_URL           server URL as seen by OML_CLI (default: 127.0.0.1:8080 inside
#                         the container, else OML_URL)
#   OML_ENROLLMENT_TOKEN  the token the server printed at first boot (default: read
#                         from `docker compose logs oml-server`)
#   OML_WORKSPACE_NAME     the workspace to create (default: crew)

set -euo pipefail
cd "$(dirname "$0")/.."

OML_URL="${OML_URL:-http://127.0.0.1:8080}"
WORKSPACE_NAME="${OML_WORKSPACE_NAME:-crew}"
if [ -n "${OML_CLI:-}" ]; then
    read -r -a CLI <<<"$OML_CLI"
    CLI_SERVER="${OML_CLI_URL:-$OML_URL}"
    IN_COMPOSE=""
else
    CLI=(docker compose exec -T oml-server oml)
    CLI_SERVER="${OML_CLI_URL:-http://127.0.0.1:8080}"
    IN_COMPOSE=1
fi

fail() {
    echo "quickstart: $*" >&2
    exit 1
}
for tool in curl jq; do
    command -v "$tool" >/dev/null || fail "$tool is required"
done
# The CLI takes one URL-and-key pair from its flags, then OML_URL and
# OML_API_KEY, then its credentials file (ADR-0039 d15). This shell's OML_URL
# is no pair, so the CLI runs without it: the claim takes --url, and every
# admin verb after it the pair the claim stored.
oml() { env -u OML_URL -u OML_API_KEY "${CLI[@]}" --json "$@"; }

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

# The secret travels on stdin, never in argv (ADR-0039 d10).
boot="$(printf '%s\n' "$TOKEN" | oml --url "$CLI_SERVER" bootstrap claim quickstart-admin --token-stdin --tenant-name quickstart --principal platform/admin)" \
    || fail "bootstrap claim refused (already claimed? the token is single-use): $boot"
workspace="$(oml workspaces create universe "$WORKSPACE_NAME" | jq -r .id)"

issue() { # key_name role used_by principal grant_role...
    local key_name="$1" role="$2" used_by="$3" principal="$4" grant issued
    local grants=()
    shift 4
    for grant in "$@"; do grants+=(--grant "$WORKSPACE_NAME:$grant"); done
    issued="$(oml keys issue "$key_name" --role "$role" --used-by "$used_by" --principal "$principal" \
        --default-workspace "$WORKSPACE_NAME" "${grants[@]}")"
    [ "$(jq -r .default_workspace_id <<<"$issued")" = "$workspace" ] \
        || fail "the workspace name $WORKSPACE_NAME did not resolve to its id $workspace: $issued"
    jq -r .value <<<"$issued"
}
agent="$(issue quickstart-agent writer agent crew/agent writer reader)"
reviewer="$(issue quickstart-reviewer reviewer person crew/reviewer reviewer)"

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
held="$(call 2 oml_write '{"text":"The quickstart wrote this memory through the agent plugin.","idempotency_key":"quickstart/1"}')"
[ "$(jq -r .result.structuredContent.state <<<"$held")" = "held" ] || fail "oml_write did not hold the write: $held"
artifact="$(jq -r .result.structuredContent.artifact_id <<<"$held")"
claim="$(jq -c '{claim_key: .result.structuredContent.claim_key}' <<<"$held")"

work="$(call 3 oml_read "$claim")"
[ "$(state_of items "$artifact" "$work")" = "held" ] || fail "oml_read does not list the write as held in its own work: $work"
own="$(call 4 oml_read '{"query":"quickstart wrote","include_unreviewed":true}')"
[ "$(state_of hits "$artifact" "$own")" = "held" ] || fail "oml_read with include_unreviewed did not recall the held write to its author: $own"

oml --url "$CLI_SERVER" --api-key "$reviewer" review next >/dev/null
oml --url "$CLI_SERVER" --api-key "$reviewer" review approve >/dev/null
approved="$(call 5 oml_read '{"state":"promoted"}')"
[ "$(state_of items "$artifact" "$approved")" = "promoted" ] || fail "oml_read with state promoted does not list the approved write: $approved"
recalled="$(call 6 oml_read '{"query":"quickstart wrote"}')"
[ "$(state_of hits "$artifact" "$recalled")" = "promoted" ] || fail "the approved memory did not come back from oml_read: $recalled"

# The object store: an upload that opens, stores its part and completes was
# written to storage.url; a server that cannot write it answers 503
# upload.storage_failed here (ADR-0004 d3, note of 2026-11-18).
upload_call() { # method path content_type body: prints the status, then the body
    curl -sS -X "$1" "$OML_URL$2" -H "authorization: Bearer $agent" \
        -H "content-type: $3" --data-binary "$4" -w '\n%{http_code}'
}
sha="$(printf 'oml' | sha256sum | cut -d' ' -f1)"
opened="$(upload_call POST /v2/uploads application/json "{\"purpose\":\"import\",\"size\":3,\"sha256\":\"$sha\"}")"
[ "$(tail -n1 <<<"$opened")" = 201 ] || fail "an upload session did not open: $opened"
upload="$(sed '$d' <<<"$opened" | jq -r .upload_id)"
stored="$(upload_call PUT "/v2/uploads/$upload/parts/1" application/octet-stream oml)"
[ "$(tail -n1 <<<"$stored")" = 200 ] || fail "the upload's part was not stored at storage.url: $stored"
completed="$(upload_call POST "/v2/uploads/$upload/complete" application/json '{}')"
[ "$(sed '$d' <<<"$completed" | jq -r .state)" = completed ] || fail "the upload did not complete: $completed"
upload_call DELETE "/v2/uploads/$upload" application/json '' >/dev/null
echo "quickstart: an upload of 3 bytes opened, stored its part at storage.url and completed (upload $upload)" >&2

cat <<EOF
OML is ready at $OML_URL. The MCP check passed on the agent's one key: write ->
held in its own work and its own recall -> approved by a different identity, as
its own work shows -> recalled. Tools served: $tools.

Workspace "$WORKSPACE_NAME": $workspace

Each key is shown once; store them now.
  agent   (writes and recalls)  $agent
  reviewer (a person reviews)    $reviewer

Give the agent its one key: in a Claude Code session, install the plugin, and
the install asks for its two settings, url $OML_URL and api_key the agent key
above, kept in the system keychain:
  /plugin marketplace add e8kor/open-memory-lab
  /plugin install oml@open-memory-lab

Nothing the agent writes reaches another key until the reviewer key approves it:
  ${CLI[*]} --url $CLI_SERVER --api-key <reviewer> review next
  ${CLI[*]} --url $CLI_SERVER --api-key <reviewer> review approve
EOF
