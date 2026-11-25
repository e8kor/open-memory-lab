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
#   3. GET /web/guardrails with that session: oml-web's 308 to the
#      workspace page's Guardrails kind (ADR-0083 d12), which answers 200
#      with In force here;
#   4. GET /web?view=guardrails: 404 with no redirect, the old URL's 303
#      deleted in v0.7.1 (ADR-0070, "The old URLs deleted");
#   5. GET /web/workspaces: 200 and the Workspaces view, from oml-web;
#   6. two acts end to end: oml-web creates <user>.proj through the API,
#      its form sent twice creating it once, then confirms putting it in
#      progress, and the confirmation's form, sent twice, runs the act once
#      and lands back on the view;
#   7. GET /web?view=workspace: 404 with no redirect;
#   8. GET /web/memories and GET /web/atlas: 200, the list and the Atlas,
#      from oml-web; GET /web?view=memories: 404 with no redirect.
#   9. GET /web: the lead's 303 to /web/keys; GET /web/keys from oml-web, and
#      one act: issuing a reader key, whose value is shown once on the
#      redirect target, carried there in a sealed cookie and never in the URL;
#  10. GET /web?view=review: 404 with no redirect; GET /web/review: oml-web's
#      308 to /web/todo (ADR-0083 d6); GET /web/todo from oml-web, and one
#      act: Next, which lands on To do's notice or on Judge.
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

code="$(curl -s -o /dev/null -D "$work/moved" -w '%{http_code}' \
    -H "Cookie: __Host-oml_session=$session" "$origin/web/guardrails")"
location="$(header "$work/moved" location)"
[ "$code $location" = "308 /web/workspace?kind=guardrail" ] || fail "GET /web/guardrails answered '$code $location'"
code="$(curl -s -o "$work/page.html" -w '%{http_code}' \
    -H "Cookie: __Host-oml_session=$session" "$origin$location")"
[ "$code" = 200 ] || fail "GET $location answered $code: $(head -c 400 "$work/page.html")"
grep -q '<h2 id="in-force">In force here</h2>' "$work/page.html" || fail "GET $location did not draw In force here"

code="$(curl -s -o /dev/null -D "$work/old" -w '%{http_code}' \
    -H "Cookie: __Host-oml_session=$session" "$origin/web?view=guardrails")"
location="$(header "$work/old" location || true)"
[ "$code $location" = "404 " ] || fail "GET /web?view=guardrails answered '$code $location'"

jar="Cookie: __Host-oml_session=$session"
field() { # <file> <name>: the value of the first input named so
    grep -o "name=\"$2\" value=\"[^\"]*\"" "$1" | head -1 | sed -e 's/.*value="//' -e 's/"$//'
}

code="$(curl -s -o "$work/workspaces.html" -w '%{http_code}' -H "$jar" "$origin/web/workspaces")"
[ "$code" = 200 ] || fail "GET /web/workspaces answered $code: $(head -c 400 "$work/workspaces.html")"
grep -q '<h1>Workspaces</h1>' "$work/workspaces.html" || fail "GET /web/workspaces did not draw the Workspaces view"
grep -q '<a href="/web/workspaces" class="on"' "$work/workspaces.html" || fail "the Workspaces tab is not the current one"
# The view names the user's root by its reference, the name every field
# takes (design notes row 20).
user="$(sed -n 's/.*Under <code>\([^<]*\)<\/code>.*/\1/p' "$work/workspaces.html")"
[ -n "$user" ] || fail "the Workspaces view names no user root"
csrf="$(field "$work/workspaces.html" csrf_token)"
[ -n "$csrf" ] || fail "the Workspaces view carries no csrf_token"
create_nonce="$(field "$work/workspaces.html" nonce)"
[ -n "$create_nonce" ] || fail "the create form carries no nonce"

for attempt in first resubmitted; do
    code="$(curl -s -o "$work/create.html" -D "$work/create" -w '%{http_code}' -X POST -H "$jar" \
        -H "Origin: $issuer" -H 'Content-Type: application/x-www-form-urlencoded' \
        --data "action=create_workspace&name=proj&csrf_token=$csrf&nonce=$create_nonce" "$origin/web/workspaces")"
    location="$(header "$work/create" location)"
    [ "$code $location" = "303 /web/workspaces?done=created" ] \
        || fail "the $attempt create answered '$code $location': $(head -c 400 "$work/create.html")"
done

code="$(curl -s -o "$work/confirm.html" -w '%{http_code}' -H "$jar" \
    "$origin/web/workspaces?workspace=$user.proj&op=mode:in_progress")"
[ "$code" = 200 ] || fail "the confirmation answered $code: $(head -c 400 "$work/confirm.html")"
grep -q 'value="workspace_mode"' "$work/confirm.html" || fail "the confirmation carries no workspace_mode form: $(head -c 400 "$work/confirm.html")"
nonce="$(field "$work/confirm.html" nonce)"
[ -n "$nonce" ] || fail "the confirmation's form carries no nonce"
for attempt in first resubmitted; do
    code="$(curl -s -o "$work/act.html" -D "$work/act" -w '%{http_code}' -X POST -H "$jar" \
        -H "Origin: $issuer" -H 'Content-Type: application/x-www-form-urlencoded' \
        --data "action=workspace_mode&workspace=$user.proj&mode=in_progress&csrf_token=$csrf&nonce=$nonce" \
        "$origin/web/workspaces")"
    location="$(header "$work/act" location)"
    [ "$code $location" = "303 /web/workspaces?done=mode+set" ] \
        || fail "the $attempt act answered '$code $location': $(head -c 400 "$work/act.html")"
done
code="$(curl -s -o "$work/after.html" -w '%{http_code}' -H "$jar" "$origin/web/workspaces?done=mode+set")"
[ "$code" = 200 ] || fail "GET /web/workspaces after the act answered $code"
grep -q "workspace=$user\.proj\">$user\.proj</a></td><td class=\"mono\">$user</td><td>development</td><td>drafts visible to everyone working here</td>" "$work/after.html" \
    || fail "the act did not make $user.proj's drafts visible to everyone working here: $(head -c 600 "$work/after.html")"

code="$(curl -s -o /dev/null -D "$work/old-workspaces" -w '%{http_code}' -H "$jar" "$origin/web?view=workspace")"
location="$(header "$work/old-workspaces" location || true)"
[ "$code $location" = "404 " ] || fail "GET /web?view=workspace answered '$code $location'"

code="$(curl -s -o "$work/memories.html" -w '%{http_code}' -H "$jar" "$origin/web/memories")"
[ "$code" = 200 ] || fail "GET /web/memories answered $code: $(head -c 400 "$work/memories.html")"
grep -q '<h1>Memories</h1>' "$work/memories.html" || fail "GET /web/memories did not draw the list"
grep -q '<a href="/web/memories" class="on"' "$work/memories.html" || fail "the Memories tab is not the current one"
code="$(curl -s -o "$work/atlas.html" -w '%{http_code}' -H "$jar" "$origin/web/atlas")"
[ "$code" = 200 ] || fail "GET /web/atlas answered $code: $(head -c 400 "$work/atlas.html")"
grep -q 'id="atlas-table"' "$work/atlas.html" || fail "GET /web/atlas did not draw the Atlas"
grep -q '<a href="/web/atlas" class="on"' "$work/atlas.html" || fail "the Atlas tab is not the current one"
code="$(curl -s -o /dev/null -D "$work/old-memories" -w '%{http_code}' -H "$jar" "$origin/web?view=memories")"
location="$(header "$work/old-memories" location || true)"
[ "$code $location" = "404 " ] || fail "GET /web?view=memories answered '$code $location'"


code="$(curl -s -o /dev/null -D "$work/landing" -w '%{http_code}' -H "$jar" "$origin/web")"
location="$(header "$work/landing" location)"
[ "$code $location" = "303 /web/keys" ] || fail "GET /web answered '$code $location'"
code="$(curl -s -o "$work/keys.html" -w '%{http_code}' -H "$jar" "$origin/web/keys")"
[ "$code" = 200 ] || fail "GET /web/keys answered $code: $(head -c 400 "$work/keys.html")"
grep -q '<h1>Keys</h1>' "$work/keys.html" || fail "GET /web/keys did not draw the Keys view"
grep -q '<a href="/web/keys" class="on"' "$work/keys.html" || fail "the Keys tab is not the current one"
# Since v0.8.5 the Keys view issues through the key builder (ADR-0096 d1)
# and draws no issue form; the issue act still takes a form's nonce, 32
# lower-case hexadecimal characters, which a browser's form would carry.
grep -q 'href="/web/keys/new"' "$work/keys.html" || fail "the Keys view does not link the key builder"
nonce="$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')"
[ "${#nonce}" = 32 ] || fail "no nonce for the issue act"
code="$(curl -s -o "$work/issue.html" -D "$work/issue" -w '%{http_code}' -X POST -H "$jar" \
    -H "Origin: $issuer" -H 'Content-Type: application/x-www-form-urlencoded' \
    --data "action=issue_key&name=ci&role=reader&csrf_token=$csrf&nonce=$nonce" "$origin/web/keys")"
location="$(header "$work/issue" location)"
[ "$code $location" = "303 /web/keys" ] || fail "POST /web/keys issue_key answered '$code $location': $(head -c 400 "$work/issue.html")"
once="$(cookie "$work/issue" __Host-oml_once)"
[ -n "$once" ] || fail "the issue carried no sealed secret to its page"
code="$(curl -s -o "$work/shown.html" -D "$work/shown" -w '%{http_code}' \
    -H "Cookie: __Host-oml_session=$session; __Host-oml_once=$once" "$origin/web/keys")"
[ "$code" = 200 ] || fail "the redirect target answered $code"
grep -q "OML_API_KEY=oml_" "$work/shown.html" || fail "the redirect target did not show the key's value once"
grep -qi '^set-cookie: __Host-oml_once=;' "$work/shown" || fail "the redirect target did not clear the sealed secret"

code="$(curl -s -o /dev/null -D "$work/old-review" -w '%{http_code}' -H "$jar" "$origin/web?view=review")"
location="$(header "$work/old-review" location || true)"
[ "$code $location" = "404 " ] || fail "GET /web?view=review answered '$code $location'"
code="$(curl -s -o /dev/null -D "$work/review" -w '%{http_code}' -H "$jar" "$origin/web/review")"
location="$(header "$work/review" location)"
[ "$code $location" = "308 /web/todo" ] || fail "GET /web/review answered '$code $location'"
code="$(curl -s -o "$work/todo.html" -w '%{http_code}' -H "$jar" "$origin/web/todo")"
[ "$code" = 200 ] || fail "GET /web/todo answered $code: $(head -c 400 "$work/todo.html")"
grep -q '<h1>To do</h1>' "$work/todo.html" || fail "GET /web/todo did not draw To do"
code="$(curl -s -o "$work/next.html" -D "$work/next" -w '%{http_code}' -X POST -H "$jar" \
    -H "Origin: $issuer" -H 'Content-Type: application/x-www-form-urlencoded' \
    --data "action=next&csrf_token=$csrf" "$origin/web/todo")"
location="$(header "$work/next" location)"
case "$code $location" in
    "303 /web/todo?done=empty" | "303 /web/todo/judge?memory="*) ;;
    *) fail "POST /web/todo next answered '$code $location': $(head -c 400 "$work/next.html")" ;;
esac
code="$(curl -s -o /dev/null -w '%{http_code}' -H "$jar" "$origin$location")"
[ "$code" = 200 ] || fail "GET $location answered $code"

echo "signed-in page: $origin served the workspace page, /web/workspaces, /web/memories, /web/atlas, /web/keys and /web/todo to a signed-in session through oml-web, moved /web/guardrails and /web/review, ran acts on the Workspaces, Keys and To do views through the API, and the old ?view= URLs are not found"
