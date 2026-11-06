<!--
id: start/first-workarea
tier: client
status: current
answers: how an administrator claims a new server, creates a workarea, issues agent and reviewer keys and grants, and rotates or revokes a key
-->

# First workarea and keys

For an administrator on a server nothing has claimed yet. You need the `oml` CLI
([install](install.md)) and a server reachable only by you until step 2 is done.

1. **Read the enrollment token.** While no tenant exists the server prints a single-use token,
   `enr_` and 48 hex digits, on stdout at every start; a restart prints a new one and voids the
   old.
2. **Claim the server** before any proxy exposes it. Whoever presents the token first receives
   the first admin key and the tenant
   ([the bootstrap race](../operations/deployment.md#claiming-the-first-admin-key-the-bootstrap-race)).

    ```sh
    printf '%s\n' "$TOKEN" | oml --url https://oml.example.com bootstrap claim ops-admin \
        --token-stdin --tenant-name acme --principal platform/you
    ```

3. **Check it.** `oml doctor` prints each health signal with a verdict and exits `1` on any fail.
4. **Create a crew and a workarea.**

    ```sh
    oml crews create metals-crew --charter-uri https://wiki.example.com/metals
    oml workareas create universe metals --crew metals-crew
    ```

5. **Issue the agent's key into a file**, never onto the screen:

    ```sh
    mkdir -p -m 700 ~/.config/oml/keys
    oml keys issue-agent claude-laptop --principal crew/agent-7 --workarea metals \
        --crew metals-crew --out ~/.config/oml/keys/claude-laptop
    ```

    Send the person the last line it prints (`oml setup claude-code --url … --key-file …`) and
    the file, then delete the file once the line has run. Inside Claude Code, the `oml-admin`
    plugin runs the same command without the key entering the conversation.
6. **Issue a reviewer's key** under another principal, because the reviewer is never the author:

    ```sh
    oml keys issue maya --role dreamer --held-by human --principal crew/maya \
        --default-workarea metals --grant metals:dreamer
    ```

7. **Look at what you built:** `oml workareas tree` and `oml keys show claude-laptop`.

From now on the agent's writes wait as `held` until `maya` approves them
([reviewing](../guides/reviewing.md)).

## The same on the web

People sign in with Google or GitHub to a user page and an admin page on the server's own
address, when an administrator has put that address on the allowlist (`oml web allow`). The user
page issues a person's own keys; the admin page covers the allowlist, workareas and keys
([ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)).

## Cut a key off or replace it

`oml deny set <key> universe` keeps a key from every workarea at its next request; a denial always
wins over a grant. To replace a key, issue its successor with `oml keys issue-agent` and deny the
old one. `oml keys revoke`, `rotate` and `list` are designed and not built; bulk rotation for a
crew is [#120](https://github.com/e8kor/open-memory-lab/issues/120). `oml keys show <key>` reads
one key by id or name. The rest of an administrator's work, including grants, crews and the
journal, is in [administering](../guides/administering.md); the verbs are in the
[CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation).
