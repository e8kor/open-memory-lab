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
#   4. GET /web?view=guardrails: the lead's 303 to /web/guardrails;
#   5. GET /web/workareas: 200 and the Workareas view, from oml-web;
#   6. two acts end to end: oml-web creates <user>.proj through the API,
#      its form sent twice creating it once, then confirms putting it in
#      progress, and the confirmation's form, sent twice, runs the act once
#      and lands back on the view;
#   7. GET /web?view=workarea: the lead's 303 to /web/workareas;
#   8. GET /web/memories and GET /web/atlas: 200, the list and the Atlas,
#      from oml-web; GET /web?view=memories: the lead's 303 to /web/memories.
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
# A helper that fails under set -e names the line, rather than exiting silently.
trap 'echo "signed-in page: line $LINENO failed: $BASH_COMMAND" >&2' ERR
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

jar="Cookie: __Host-oml_session=$session"
field() { # <file> <name>: the value of the first input named so
    grep -o "name=\"$2\" value=\"[^\"]*\"" "$1" | head -1 | sed -e 's/.*value="//' -e 's/"$//'
}

code="$(curl -s -o "$work/workareas.html" -w '%{http_code}' -H "$jar" "$origin/web/workareas")"
[ "$code" = 200 ] || fail "GET /web/workareas answered $code: $(head -c 400 "$work/workareas.html")"
grep -q '<h1>Workareas</h1>' "$work/workareas.html" || fail "GET /web/workareas did not draw the Workareas view"
grep -q '<a href="/web/workareas" class="on"' "$work/workareas.html" || fail "the Workareas tab is not the current one"
user="$(sed -n 's/.*Under <code>universe\.\([^<]*\)<\/code>.*/\1/p' "$work/workareas.html")"
[ -n "$user" ] || fail "the Workareas view names no user root"
csrf="$(field "$work/workareas.html" csrf_token)"
[ -n "$csrf" ] || fail "the Workareas view carries no csrf_token"
create_nonce="$(field "$work/workareas.html" nonce)"
[ -n "$create_nonce" ] || fail "the create form carries no nonce"

for attempt in first resubmitted; do
    code="$(curl -s -o "$work/create.html" -D "$work/create" -w '%{http_code}' -X POST -H "$jar" \
        -H "Origin: $issuer" -H 'Content-Type: application/x-www-form-urlencoded' \
        --data "action=create_workarea&name=proj&csrf_token=$csrf&nonce=$create_nonce" "$origin/web/workareas")"
    location="$(header "$work/create" location)"
    [ "$code $location" = "303 /web/workareas?done=created" ] \
        || fail "the $attempt create answered '$code $location': $(head -c 400 "$work/create.html")"
done

code="$(curl -s -o "$work/confirm.html" -w '%{http_code}' -H "$jar" \
    "$origin/web/workareas?workarea=$user.proj&op=mode:in_progress")"
[ "$code" = 200 ] || fail "the confirmation answered $code: $(head -c 400 "$work/confirm.html")"
grep -q 'value="workarea_mode"' "$work/confirm.html" || fail "the confirmation carries no workarea_mode form: $(head -c 400 "$work/confirm.html")"
nonce="$(field "$work/confirm.html" nonce)"
[ -n "$nonce" ] || fail "the confirmation's form carries no nonce"
for attempt in first resubmitted; do
    code="$(curl -s -o "$work/act.html" -D "$work/act" -w '%{http_code}' -X POST -H "$jar" \
        -H "Origin: $issuer" -H 'Content-Type: application/x-www-form-urlencoded' \
        --data "action=workarea_mode&workarea=$user.proj&mode=in_progress&csrf_token=$csrf&nonce=$nonce" \
        "$origin/web/workareas")"
    location="$(header "$work/act" location)"
    [ "$code $location" = "303 /web/workareas?done=mode+set" ] \
        || fail "the $attempt act answered '$code $location': $(head -c 400 "$work/act.html")"
done
code="$(curl -s -o "$work/after.html" -w '%{http_code}' -H "$jar" "$origin/web/workareas?done=mode+set")"
[ "$code" = 200 ] || fail "GET /web/workareas after the act answered $code"
grep -q "universe\.$user\.proj</td><td class=\"mono\">$user\.proj</td><td>development</td><td>in progress</td>" "$work/after.html" \
    || fail "the act did not put $user.proj in progress: $(head -c 600 "$work/after.html")"

code="$(curl -s -o /dev/null -D "$work/old-workareas" -w '%{http_code}' -H "$jar" "$origin/web?view=workarea")"
location="$(header "$work/old-workareas" location)"
[ "$code $location" = "303 /web/workareas" ] || fail "GET /web?view=workarea answered '$code $location'"

code="$(curl -s -o "$work/memories.html" -w '%{http_code}' -H "$jar" "$origin/web/memories")"
[ "$code" = 200 ] || fail "GET /web/memories answered $code: $(head -c 400 "$work/memories.html")"
grep -q '<h1>Memories</h1>' "$work/memories.html" || fail "GET /web/memories did not draw the list"
grep -q '<a href="/web/memories" class="on"' "$work/memories.html" || fail "the Memories tab is not the current one"
code="$(curl -s -o "$work/atlas.html" -w '%{http_code}' -H "$jar" "$origin/web/atlas")"
[ "$code" = 200 ] || fail "GET /web/atlas answered $code: $(head -c 400 "$work/atlas.html")"
grep -q 'id="atlas-table"' "$work/atlas.html" || fail "GET /web/atlas did not draw the Atlas"
grep -q '<a href="/web/atlas" class="on"' "$work/atlas.html" || fail "the Atlas tab is not the current one"
code="$(curl -s -o /dev/null -D "$work/old-memories" -w '%{http_code}' -H "$jar" "$origin/web?view=memories")"
location="$(header "$work/old-memories" location)"
[ "$code $location" = "303 /web/memories" ] || fail "GET /web?view=memories answered '$code $location'"

echo "signed-in page: $origin served /web/guardrails, /web/workareas, /web/memories and /web/atlas to a signed-in session through oml-web, ran two acts through the API, and the old URLs redirect there"
