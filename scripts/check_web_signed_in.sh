#!/usr/bin/env bash
# A signed-in person loads a page oml-web serves through the router
# (ADR-0070 d4, d7 (b)): the session cookie crosses the router to oml-web,
# oml-web's capacity credential and web client identity reach the lead's API,
# and the page renders. The stack runs a lead built with the e2e-sign-in
# feature and OML_E2E_SIGN_IN_EMAIL set, oauth.issuer and a GitHub client, and
# the address is on the allowlist (crates/bins/oml-server-lead/src/e2e.rs).
#
#   1. POST /web/sign-in for GitHub, from the issuer's origin, next the page;
#   2. the provider's callback with the sealed state, which opens a session;
#   3. GET /web/guardrails with that session: 200 and the view's content;
#   4. GET /web?view=guardrails: the lead's 303 to /web/guardrails.
#
# Usage: scripts/check_web_signed_in.sh <origin the router answers> <oauth.issuer>
set -euo pipefail

origin="$1"
issuer="$2"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() {
    echo "signed-in page: $*" >&2
    exit 1
}
header() { # <file> <lower-case name>: the first value, without CR
    grep -i "^$2:" "$1" | head -1 | cut -d' ' -f2- | tr -d '\r'
}
cookie() { # <file> <name>: the value a Set-Cookie gives it
    grep -i "^set-cookie: $2=" "$1" | head -1 | sed -e "s/^[^=]*=//" -e 's/;.*//' | tr -d '\r'
}

code="$(curl -s -o /dev/null -D "$work/start" -w '%{http_code}' -X POST \
    -H "Origin: $issuer" -H 'Content-Type: application/x-www-form-urlencoded' \
    --data 'provider=github&next=%2Fweb%2Fguardrails' "$origin/web/sign-in")"
[ "$code" = 303 ] || fail "POST /web/sign-in answered $code"
state="$(header "$work/start" location | sed -n 's/.*[?&]state=\([^&]*\).*/\1/p')"
sealed="$(cookie "$work/start" __Host-oml_signin)"
[ -n "$state" ] && [ -n "$sealed" ] || fail "the sign-in start carried no state or sign-in cookie"

code="$(curl -s -o "$work/callback.html" -D "$work/callback" -w '%{http_code}' \
    -H "Cookie: __Host-oml_signin=$sealed" "$origin/web/callback/github?code=e2e&state=$state")"
[ "$code" = 303 ] || fail "the callback answered $code: $(head -c 400 "$work/callback.html")"
session="$(cookie "$work/callback" __Host-oml_session)"
[ -n "$session" ] || fail "the callback opened no session"

code="$(curl -s -o "$work/page.html" -D "$work/page" -w '%{http_code}' \
    -H "Cookie: __Host-oml_session=$session" "$origin/web/guardrails")"
[ "$code" = 200 ] || fail "GET /web/guardrails answered $code: $(head -c 400 "$work/page.html")"
grep -q 'Rules in force at' "$work/page.html" || fail "GET /web/guardrails did not draw the Guardrails view"
grep -q '<a href="/web/guardrails" class="on"' "$work/page.html" || fail "the Guardrails tab is not the current one"

code="$(curl -s -o /dev/null -D "$work/old" -w '%{http_code}' \
    -H "Cookie: __Host-oml_session=$session" "$origin/web?view=guardrails")"
location="$(header "$work/old" location)"
[ "$code $location" = "303 /web/guardrails" ] || fail "GET /web?view=guardrails answered '$code $location'"

echo "signed-in page: $origin served /web/guardrails to a signed-in session through oml-web, and the old URL redirects there"
