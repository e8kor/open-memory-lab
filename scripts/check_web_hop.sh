#!/usr/bin/env bash
# The web hop of a running stack (ADR-0070 d4), through its one origin:
#   - the public /readyz is the lead's, and the lead accepts oml-web's
#     client identity (web_client is not none);
#   - the moved pages, GET /web/guardrails, /web/workareas, /web/memories
#     and /web/atlas, reach
#     oml-web, which sends a browser with no session to the lead's
#     /web/sign-in carrying the page as next;
#   - the rest of the command line, oml-web --doctor run where oml-web runs,
#     exits 0.
# Used by .github/workflows/ci.yml (job compose) for docker-compose.yml and for
# the stack `oml quickstart` writes.
#
# Usage: scripts/check_web_hop.sh <origin> <doctor command...>
set -euo pipefail

origin="$1"
shift

fail() {
    echo "web hop: $*" >&2
    exit 1
}

ready=""
for _ in $(seq 1 60); do
    ready="$(curl -fsS "$origin/readyz" 2>/dev/null)" && break
    sleep 1
done
[ -n "$ready" ] || fail "$origin/readyz never answered ready"
web_client="$(jq -r '.web_client // "absent"' <<<"$ready")"
[ "$web_client" = "secret" ] || fail "the lead's /readyz names web_client $web_client, not secret"

# oml-web may start after the router: wait for its answer rather than the
# router's 502.
got=""
want="303 $origin/web/sign-in?next=%2Fweb%2Fguardrails"
for _ in $(seq 1 60); do
    got="$(curl -s -o /dev/null -w '%{http_code} %{redirect_url}' "$origin/web/guardrails")"
    [ "$got" = "$want" ] && break
    sleep 1
done
[ "$got" = "$want" ] || fail "GET /web/guardrails answered '$got', not '$want'"
got="$(curl -s -o /dev/null -w '%{http_code} %{redirect_url}' "$origin/web/workareas")"
want="303 $origin/web/sign-in?next=%2Fweb%2Fworkareas"
[ "$got" = "$want" ] || fail "GET /web/workareas answered '$got', not '$want'"
for page in memories atlas; do
    got="$(curl -s -o /dev/null -w '%{http_code} %{redirect_url}' "$origin/web/$page")"
    want="303 $origin/web/sign-in?next=%2Fweb%2F$page"
    [ "$got" = "$want" ] || fail "GET /web/$page answered '$got', not '$want'"
done

"$@" || fail "oml-web --doctor failed"
echo "web hop: $origin routes the moved pages to oml-web, and oml-web reaches the lead"
