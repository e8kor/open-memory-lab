<!--
id: adr/ADR-0076-releases-ship-no-default-server-and-setup-asks-for-the-server-or-reads-it-from-the-key-or-the-environment
tier: engineering
status: accepted design · 2026-11-11
answers: why OML's releases stop naming a default server; where a client gets its server instead (the setup prompt, the pair beside the key, the environment, the issuing server's own pages); what the plugin, the hooks, oml setup claude-code, the agent kit and the web pages do with no server named; how oml sync's consent works without a host to compare against; what a client with no server configured prints; how installs and released plugins that rely on the old default move; which hard-coded occurrences change; which tests must exist; what it amends in ADR-0043, ADR-0040, ADR-0041 and ADR-0057
-->

# ADR-0076 — OML releases name no server: setup asks for one or takes it beside the key, and a key goes only to a server its holder named

**Status:** accepted design · 2026-11-11, by the operator ("Accept, then build"), with three of the four readings put to them accepted and the fourth reversed in d9 · proposed the same day as #265

**Release:** not assigned; the build lands after v0.7.0 is cut, with the production split (#194)

**Amends:** ADR-0043 (d1, d2, d3, d4, d6, d8 replaced; d7 narrowed; its Consequences' "a move of
the maintainer's server is a release"), ADR-0039 (d15: the setup exception and the kit clause
ADR-0043 added are withdrawn, so the pair rule holds with no exception and the kit reads
`OML_URL` where a tool expands it; the no-pair refusal's message gains the `no_server` name),
ADR-0040 (d1 and d3 step 1 restored as accepted on 2026-11-04, before ADR-0043), ADR-0041 (d7
step 1), ADR-0057 (d6's public-server clause, d17's public-server doctor line)

## Context

[ADR-0043](ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md) made
`https://memory.doublegate.io`, the server the maintainer runs, the default of every integration a
release ships: the Claude Code plugin's `url` option, the hooks reading a plugin configured with
its key alone, `oml setup claude-code` given a key and no URL, every agent kit configuration, and
the web pages' setup text. [ADR-0057](ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents.md)
d6 then keyed `oml sync`'s consent to the same host.

On 2026-11-11 the operator decided that the open-source repository ships **no default server URL**,
choosing "no default; ask on setup": setup asks for the server or reads it from the key or the
environment; the hosted service's own onboarding supplies its URL; released plugins keep working
when given an explicit URL. The hosted service, its landing and its runbooks leave this repository
(issue #194); a default naming that service would be the one place where every release still
carries it.

The operator's rule of 2026-11-03 stands and bounds this record: a client starts from a URL and a
key, and the server derives everything else ([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md)
d15). This record removes the value the URL defaulted to. It adds no third setting.

Two facts shape the decisions. A key value encodes no server, role or tenant
([roles and keys](../architecture/roles-and-keys.md#key-format)), so "the server from the key" can
only mean the pair as the issuing server hands it out: `oml keys issue-agent` prints `oml setup
claude-code --url <server> --key-file <file>`, and the web page shows its own address beside a new
key. And Claude Code fills a plugin's MCP configuration from saved values merged over each option's
`default`, while a hook sees saved values alone (ADR-0043's Context, read from Claude Code 2.1.312);
an option with no default and no saved value reaches neither. Decided, not built.

## Decision

- **d1 — No surface this repository releases names a server it was not given.** The CLI, the
  `oml` plugin, the hooks, the agent kit, the web pages and the release checks carry no default
  server URL, no constant naming one, and no host list standing in for one. The four constants
  ADR-0043 d1 introduced or that later code copied (`DEFAULT_URL` in
  `crates/bins/oml-cli/src/setup.rs` and `scripts/package_agent_artifacts.py`, `PUBLIC_SERVER` in
  `crates/bins/oml-cli/src/sync/source.rs`, `DEFAULT_SERVER` in
  `crates/clients/oml-web/src/keys.rs` and `crates/transports/oml-transport-http/src/web/views.rs`)
  are removed, with every line listed under "Occurrences to change". A key goes only to a URL its
  holder named beside it, which is ADR-0039 d15's pair rule with no exception. The cost, carried
  openly: a user of a hosted server sets two values instead of one, and the hosted service's
  onboarding must hand out its URL (d6).

- **d2 — The plugin's `url` option is required and has no default again (replaces ADR-0043 d2,
  restores ADR-0040 d1).**

    ```json
    "url": {
      "type": "string",
      "title": "OML server URL",
      "description": "Your Open Memory Lab server, without /mcp, as its sign-up page or administrator gave it to you.",
      "required": true
    }
    ```

    Claude Code's configuration dialog does not accept an empty value for a required option
    ([plugin manifest reference](https://code.claude.com/docs/en/plugins-reference), "User
    configuration", as ADR-0043 quotes it, fetched 2026-11-04), so the dialog is where the plugin
    asks. `api_key` and `chat_workarea` are unchanged; `.mcp.json` is unchanged.

- **d3 — A plugin configured with its key alone is a plugin not configured, in every hook
  (replaces ADR-0043 d3, restores ADR-0040 d3 step 1).** When `CLAUDE_PLUGIN_OPTION_API_KEY` is
  set and `CLAUDE_PLUGIN_OPTION_URL` is unset or empty, `oml hook` sends nothing and fails open
  (ADR-0029 d5) with the line of d8. The environment pair is unchanged: `OML_API_KEY` without
  `OML_URL` fails open naming `OML_URL`.

- **d4 — `oml setup claude-code` takes the server from the pair, or asks for it on a terminal, and
  otherwise refuses before it sends anything (replaces ADR-0043 d4, amends ADR-0041 d7 step 1).**
  In order:
    1. **The pair**, as ADR-0039 d15 resolves it: `--url` with `--api-key` or `--key-file`;
       `OML_URL` with `OML_API_KEY`; the credentials file's `url` with its `api_key`. This is
       "from the environment", and, for a key that `oml keys issue-agent` or a web page issued,
       "from the key": the line those print carries `--url` (d6).
    2. **The prompt.** A key named without a URL: `--key-file` or `--api-key` without `--url`,
       `OML_API_KEY` without `OML_URL`, or a credentials file holding `api_key` and no `url`.
       "On a terminal" means stdin and stderr are both terminals, the test `oml login` already
       uses for its key (`read_secret`, `IsTerminal`). The verb prints `OML server URL (without
       /mcp):` to stderr and reads one line from stdin, with echo, since a URL is not a secret.
       It shows no suggested value. A non-empty answer is the URL, used exactly as `--url` would
       be; `whoami` then checks the pair before anything is installed or printed (ADR-0041 d7
       step 2).
    3. **The refusal.** Off a terminal, or an empty answer: refused as `setup.no_server` (d8),
       exit 2, nothing sent and nothing installed. A script or CI job passes both values without
       a prompt: `--url <server> --key-file <file>`, or `OML_URL` and `OML_API_KEY` exported
       together.

    `--print` takes the same three steps and then prints the commands instead of running them, so
    a printed configure line always carries a URL someone named, never a placeholder.

    A `--key-file` key is never paired with `OML_URL` or the credentials file's `url`: each is
    another source, and ADR-0039 d15 sends a key only to a URL named beside it. When `OML_URL` is
    set and a `--key-file` key came without `--url`, the prompt and the refusal both add `OML_URL
    is set but is never paired with --key-file; pass it as --url`, so the question is not a
    surprise. A URL named without a key is still refused. The configure line the verb runs or
    prints carries the URL it used, as before.

- **d5 — Every other CLI verb behaves as before (ADR-0043 d5 stands); only its refusal's message
  changes.** `oml login <url>` keeps its positional; flags, environment and the credentials file
  each give both values or are refused. The refusal is ADR-0039 d15's, unchanged in when it fires
  and in its exit code; its text gains the shared reason name `no_server` and `oml login <server>`
  (d8), so every surface names the same cause.
  `oml quickstart` keeps writing the local server's own URL into the credentials file it creates
  (ADR-0041 d9): a server the user started is a server they named.

- **d6 — The server that issues a key hands out its own URL beside it; nothing else does.** This
  is the hosted service's onboarding, and every self-hosted server's:
    - `oml keys issue-agent` prints the configure and setup lines with the URL the admin's own
      pair reached (unchanged; `crates/bins/oml-cli/src/issue.rs`).
    - The web pages show, beside a new key and on the setup view, the server's `oauth.issuer`;
      `oml-web` shows the API's issuer, else its own `web.public_url`. When neither is set, the
      page names the unset setting (d8) and shows no URL. The page never says "the plugins'
      default": there is none.
    - A hosted service's landing, sign-up mail and documentation name its URL. They live outside
      this repository (#194).

- **d7 — The agent kit names no reachable server (replaces ADR-0043 d6).** Cursor and Gemini CLI,
  which expand a variable inside a URL, read `OML_URL` again, each in its own syntax, as the kit
  did before ADR-0043 (`scripts/package_agent_artifacts.py` at `48a9ec15`, lines 125–126):
    - **Cursor:** `${env:OML_URL}/mcp`. Cursor resolves `${env:NAME}` in `url` and `headers`
      ([Cursor MCP documentation](https://cursor.com/docs/context/mcp), fetched 2026-11-04, as
      `kit/README.md` cites it); the kit's Cursor header is already `Bearer ${env:OML_API_KEY}`.
    - **Gemini CLI:** `${OML_URL}/mcp`. Gemini CLI resolves `$VAR` or `${VAR}` in string values of
      `settings.json`
      ([configuration reference](https://github.com/google-gemini/gemini-cli/blob/main/docs/reference/configuration.md),
      fetched 2026-11-04, as `kit/README.md` cites it); the kit's Gemini header is already
      `Bearer ${OML_API_KEY}`.

  With `OML_URL` unset, the URL is `/mcp` or the unexpanded text, depending on how each tool
  treats an unset variable. Neither names a host, so nothing can be sent. Which of the two each
  tool produces is **unverified**, and the build records one run of each. OpenAI Codex
  CLI, VS Code and Claude Desktop, which document no variable inside a URL, carry
  `https://oml-server.invalid/mcp`. `.invalid` is reserved and never resolves (RFC 6761 §6.4), so
  a configuration copied without its one edit fails at name resolution and the key never leaves
  the machine. `kit/README.md` names the edit for each tool and says that the URL comes from the
  server that issued the key. The cost: Codex CLI, VS Code and Claude Desktop users always make
  one edit; Cursor and Gemini CLI users export `OML_URL`, which the Gemini CLI hook already needs.

- **d8 — A client with no server configured fails closed, sends nothing, and says why by name.**
  One reason name, `no_server`, appears on every surface:

    | Surface | What it prints | Outcome |
    |---|---|---|
    | `oml setup claude-code`, no URL and no terminal, or an empty answer | `error: refused setup.no_server: a key was given without a server URL; pass --url <server> (without /mcp), or export OML_URL beside OML_API_KEY; nothing was sent` | exit 2; nothing sent or installed |
    | any other CLI verb with no pair | ADR-0039 d15's existing refusal, its message gaining the `no_server:` prefix and `oml login <server>` | exit 2; nothing sent |
    | `oml hook` (every verb) with the key option alone | `no_server: CLAUDE_PLUGIN_OPTION_URL is not set, so no context was loaded; set the oml plugin's url option to your server (claude plugin configure oml@open-memory-lab, or oml setup claude-code --url <server>)` | exit 0, one stderr line, nothing sent (ADR-0029 d5) |
    | `oml doctor`, no pair | `fail no_server: no server URL is configured (--url, OML_URL or the credentials file); nothing was probed` in place of today's three `not probed (no URL)` lines | exit 1 |
    | `oml doctor`, run with the plugin's variables exported and the key option alone | `warn plugin_no_server: the oml plugin has a key and no url; since this release its url has no default, so its tools reach no server and its hooks load nothing` | exit 0 |
    | web setup view and new-key panel, no `oauth.issuer` (lead) or `web.public_url` (oml-web) | `This server's address is not configured (no_server: set oauth.issuer); ask its operator for the URL to give your agents.` | page served; no URL shown |
    | `oml doctor` with an admin key against a lead with no `oauth.issuer` | `warn server_origin: the server names no public origin (oauth.issuer unset), so its pages cannot show the URL agents set beside their keys` | exit 0 |
    | the plugin's MCP tools with no saved `url` | whatever Claude Code shows for an unresolved `${user_config.url}`; **unverified** for Claude Code 2.1.312 and later, no run has recorded it. The hook line above and `oml doctor` name the cause | no tools served |

    Each line is a stable string a test in d10 matches (tests 4, 6, 9, 10, 17 to 20 and 24), and each
    is in `oml doctor`'s reference page in the same change. The MCP-tool row is the exception:
    what it shows is Claude Code's, so it is recorded by a run rather than tested.

- **d9 — `oml sync` asks consent of a server that declares itself shared, not of one host
  (amends ADR-0057 d6 and d17).** ADR-0057 d6's disclosure stays: on a server whose admins are not
  the caller's, they can read submitted memories, and supersession or retirement does not erase
  submitted text or anchors. What triggers it changes:
    - **The server says so.** `GET /v2/whoami` always answers a boolean `shared`: `true` when the
      new setting `server.shared` is `true` or `signup.mode` is `open`
      ([ADR-0068](ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md)),
      else `false`. `server.shared` defaults to `false`; a hosted service sets it in its own
      configuration, before, or in the same deploy as, its upgrade past this change (see
      Consequences, "Rollout order").
      `shared` is the server's own statement about itself: it makes the disclosure reach the
      people who need it, and it is not a security control. A server whose admins mean harm can
      answer `false`; the user named that server, and the host check it replaces protected one
      host only.
    - **Optional in the schema: the operator's clarification of 2026-11-11.** A server from this
      release always sends `shared`, but `docs/api/openapi.yaml` marks it optional, so the
      generated SDKs read an answer without it, from an older server, as not shared, as the CLI
      does.
    - **A server that does not say counts as not shared: the operator's choice of 2026-11-11.** A
      `whoami` answer without `shared` comes from a server older than this change, and the run asks
      no consent of it. The record proposed the opposite (ask consent of a server that does not
      say); the operator did not accept that reading. The consequence, plainly: an older server
      whose admins are not the caller's, the maintainer's hosted server among them until it
      upgrades, gets no disclosure from a new CLI until it upgrades and sets `server.shared`. The
      safeguard is the hosted server's rollout order (Consequences): it sets `server.shared = true`
      before, or in the same deploy as, its upgrade, and that deploy should follow the release that builds
      this as closely as it can, since a new CLI pointed at it before then syncs without the
      warning. A self-hoster on an older server is never asked.
    - **Consent is per origin.** `--allow-public-server` keeps its name. The first opt-in records
      the server's origin (scheme, lowercased host, port) as one value of the multi-valued git
      config `oml.allowPublicServer` in the source clone; a later run is allowed when its origin is
      among the values. Consent to one server never covers another. A value of `true`, which
      ADR-0057 d6 wrote and which named no server, is not consent.
    - **Doctor.** ADR-0057 d17's line becomes `warn sync_shared_server: <url> declares itself
      shared, and consent is not recorded for
      this origin in this clone; run oml sync with --allow-public-server to opt in`, and a clone
      holding the old `true` gets `warn sync_consent_unbound: oml.allowPublicServer=true names no
      server and is no longer read; run oml sync --allow-public-server once against the server you
      mean`.

- **d10 — The tests hold d1 to d9 (replaces ADR-0043 d8).** Each is named after the sentence it
  proves; "sends nothing" is asserted on a recording test server that counts every request, never
  inferred from an exit code.

    **Absence, through the real surfaces**
    1. `scripts/check_no_default_server.py` (new, in the practices workflow) scans every tracked
       file except `docs/adr/`, `docs/changelog.md`, `changes/`, `docs/legal/`, `site/`,
       `scripts/tests/test_site_build.py` and `crates/bins/oml-cli/tests/fixtures/`. It fails on:
       the hosted service's names (the pattern `doublegate\.io`, and the phrases "maintainer's
       public server" and "the public server", case-insensitive); a constant named
       `DEFAULT_URL`, `DEFAULT_SERVER` or `PUBLIC_SERVER`; and a `default` on the plugin's `url`
       option. Docs and `README.md` are in scope, so a page cannot bring the default back. The
       pattern is narrow on purpose: an allowlist of hosts over `crates/` would trip on scan-rule
       fixtures and cited references.
    2. `scripts/check_plugin.py`: the `url` option is `required` and declares no `default`;
       `api_key` is unchanged.
    3. `scripts/check_agent_kit.py`: Cursor reads `${env:OML_URL}/mcp`; Gemini CLI reads
       `${OML_URL}/mcp`; Codex CLI, VS Code and Claude Desktop carry
       `https://oml-server.invalid/mcp`; the kit README names the edit for each, before the copy
       step.

    **CLI (`crates/bins/oml-cli/tests/setup_claude_code.rs`, `hook.rs` unit tests)**

    4. `setup_with_a_key_and_no_url_off_a_terminal_is_refused_as_no_server_and_sends_nothing`
    5. `setup_with_a_key_and_no_url_on_a_terminal_sends_the_key_only_to_the_url_typed`, run on a
       pseudo-terminal from the `expectrl` crate, a new dev-dependency that `cargo deny` checks;
       the test is `#[cfg(unix)]`
    6. `setup_with_an_empty_answer_at_the_prompt_is_refused_as_no_server_and_sends_nothing`
    7. `setup_never_pairs_a_key_file_with_oml_url_from_the_environment`
    8. `the_configure_line_carries_the_url_the_user_named`
    9. `a_hook_with_the_key_option_alone_names_the_url_option_and_sends_nothing` (replaces the
       ADR-0043 d3 test at `hook.rs` 1403–1430)
    10. `doctor_with_no_pair_prints_fail_no_server_and_probes_nothing`
    19. `a_verb_with_no_pair_is_refused_with_the_no_server_name_and_oml_login`
    20. `doctor_with_the_plugin_key_option_alone_warns_plugin_no_server`
    21. `setup_with_oml_url_set_and_a_key_file_names_why_it_is_not_paired`
    22. `a_credentials_file_with_a_key_and_no_url_prompts_on_a_terminal_and_is_refused_off_one`

    **`oml sync` (`crates/bins/oml-cli/tests/sync_git.rs`)**

    11. `sync_to_a_server_that_declares_itself_shared_is_refused_without_consent`
    12. `sync_to_a_server_whose_whoami_omits_shared_asks_no_consent`
    13. `sync_to_a_server_that_declares_itself_not_shared_asks_nothing`
    14. `consent_recorded_for_one_origin_does_not_cover_another`
    15. `a_recorded_true_is_not_consent_and_doctor_names_it` (the `sync_consent_unbound` line)
    23. `doctor_names_sync_shared_server_when_consent_for_that_origin_is_missing`

    **Server and pages**

    16. `whoami_says_shared_when_server_shared_is_set_or_sign_up_is_open` (`oml-transport-http`,
        over the in-memory app; `server.shared`'s parsing in `oml-server-lead`'s unit tests), and
        `docs/api/openapi.yaml` declares `shared` on `Whoami`, optional; the Python and TypeScript SDK models parse a whoami without it
    17. `the_setup_view_shows_the_servers_own_issuer_and_names_no_other_server`
        (`oml-transport-http`)
    18. `a_new_key_is_shown_beside_the_apis_issuer_and_with_none_beside_a_named_no_server_line`
        (`oml-web`)
    24. `doctor_with_an_admin_key_warns_server_origin_when_oauth_issuer_is_unset`
        (`crates/bins/oml-cli/tests/no_server.rs`, against a fixture lead)

    Numbers 19 to 24 were added in review and sit with their group; the numbering is a handle,
    not an order.

## Consequences

- **ADR-0043 is amended in almost every decision.** Its Status line gains `· amended by ADR-0076`; its text
  stays as history. ADR-0039,
  ADR-0040, ADR-0041 and ADR-0057 gain the same back-pointer. The D14 row of the [D-register](index.md) names this
  record beside ADR-0043.
- **Who keeps the old name alive.** ADR-0043's Consequences made a move of the maintainer's server
  a release. Without a default, a move is the hosted service's own concern: installs that saved
  its URL, and released plugins that default to it, keep sending there, and its operator keeps the
  name answering or redirects it for as long as they are in use. That obligation belongs to the
  hosted service's runbooks, outside this repository.
- **Rollout order, the one safeguard for the hosted server.** A hosted service that is not open
  for sign-up sets `server.shared = true` before, or in the same deploy as, its upgrade past this change, and
  makes that deploy as soon after the release as it can. Before it upgrades it omits `shared`, and
  by the operator's choice in d9 new CLIs read that as not shared and sync to it with no
  disclosure; after it upgrades without the setting it answers `shared: false`, with the same
  result. Either way ADR-0057 d6's warning is missing until the setting is live. The release notes
  of the release that builds this name the setting first, and the hosted service's own runbook
  carries the step.
- **Released plugins and kits, v0.4.1 to v0.6.3 and whatever ships before this is built**, keep the
  default they shipped; no release can change it. Given an explicit `url` they reach any server,
  as they always did. Their hooks run the `oml` on `PATH`, so a user who upgrades the CLI and keeps
  an older plugin with no saved `url` gets tools that still reach the old default and hooks that
  print d8's line. `oml setup claude-code` pins the marketplace to the CLI's own tag (ADR-0041 d7
  step 4), so upgrading the plugin through setup also asks for the URL (d4).
- **Migration for an existing user.** One command, run once:
  `oml setup claude-code --url <server> --key-file <file>`, or `claude plugin configure
  oml@open-memory-lab` and set `url`. A script or CI job that ran `oml setup claude-code` with a
  key alone is now refused off a terminal; it passes `--url` beside `--key-file`, or exports
  `OML_URL` with `OML_API_KEY`. A kit user replaces the URL in the copied file. A sync user
  repeats `--allow-public-server` once per server they consented to before. The credentials file
  is unchanged. The Python and TypeScript SDKs are regenerated for `Whoami.shared`, which is
  optional, so they still read an older server's whoami. A hosted operator sets `server.shared` first (Rollout
  order). The release notes of the release that builds this carry these lines; the hosted service
  tells its own users.
- **Pages that change with the build** (the D-register's owners): the repository README's quick
  start, which leads with `oml quickstart` and a server you run, and says a hosted server's sign-up
  gives you its URL; `docs/start/install.md` and `docs/start/index.md`; the Claude Code,
  other-harnesses, claude.ai-and-mobile and get-an-agent-key guides; `docs/help/troubleshooting-client.md`;
  `docs/use/sync.md` and `docs/use/chat-memory.md`; `docs/faq.md`; `docs/reference/limits.md`;
  `plugins/oml/README.md` and `kit/README.md`; `docs/cli/reference.md`, regenerated from clap;
  `docs/operations/configuration.md` gains `server.shared`.
- **Cowork.** Claude Code's Cowork ignores a plugin MCP server when a referenced option has no
  default ([platform support](https://claude.com/docs/plugins/platform-support), as ADR-0043 cites
  it). `api_key` already has none, so Cowork loads what it loaded before.
- **The two-setting rule is kept.** A client still starts from a URL and a key; the URL now always
  comes from the person or the issuing server.
- **No issue is opened** for an unmeasured quantity: the cost of d1 is one more value per install,
  not a number. The build is one change: the code, the checks, the pages and a changelog fragment
  together, after the hosted service's onboarding names its URL (d6), so no hosted user is left
  without one.

### Occurrences to change

Read on `origin/main` at `694a35d2` (2026-11-11). Frozen ADR copies under
`crates/bins/oml-cli/tests/fixtures/sync/` stay as they are: sync tests read them as data.

| File:line | What | Change |
|---|---|---|
| `crates/bins/oml-cli/src/setup.rs:19-45` | `DEFAULT_URL`, `DEFAULT_NOTICE`, `DEFAULT_REFUSED`, `url_beside_key` | remove; d4's prompt and `setup.no_server` |
| `crates/bins/oml-cli/src/setup.rs:310-345` | unit tests of the default | replace with d10 tests 4–8 |
| `crates/bins/oml-cli/src/main.rs:235, 242` | `oml setup claude-code` help naming the default | d4's sources and prompt |
| `crates/bins/oml-cli/src/main.rs:824-828` | `--allow-public-server` help, "the maintainer's public server" | "a server that declares itself shared" |
| `crates/bins/oml-cli/src/main.rs:1155` | `NO_PAIR` | d8's `no_server` prefix and `oml login <server>` |
| `crates/bins/oml-cli/src/main.rs:2157-2177, 2179, 2218, 2265` | `authorize_public_sync` and its two callers | d9's `whoami.shared` and per-origin consent |
| `crates/bins/oml-cli/src/main.rs:4335-4395` | `setup_pair` defaulting | d4 |
| `crates/bins/oml-cli/src/main.rs:4664-`, `4912, 4923, 4989, 5028, 5345` | doctor with no URL | d8's `fail no_server`, `warn plugin_no_server`, `warn server_origin` |
| `crates/bins/oml-cli/src/hook.rs:200-213, 240-245` | `resolve_pair`'s doc comment naming ADR-0043 d3; key option alone takes `DEFAULT_URL` | d3 |
| `crates/bins/oml-cli/src/hook.rs:1403-1431` | test of ADR-0043 d3 | d10 test 9 |
| `crates/bins/oml-cli/src/sync/source.rs:358-442` | `PUBLIC_SERVER`, `is_public_server`, `ALLOW_PUBLIC_CONFIG = true` | d9 |
| `crates/bins/oml-cli/src/sync/doctor.rs:2-3, 26-34` | `sync_public_server` | d9's two doctor lines |
| `crates/bins/oml-cli/tests/setup_claude_code.rs:330` | asserts the host's absence in one path | d10 tests 4–8 |
| `crates/bins/oml-cli/tests/sync_git.rs:14, 281-400` | host matching and `true` consent | d10 tests 11–15 |
| `crates/clients/oml-web/src/keys.rs:36-39, 397-405` | `DEFAULT_SERVER` fallback | d6, d8 |
| `crates/transports/oml-transport-http/src/web/views.rs:30-32, 318-327` | `DEFAULT_SERVER` and "the plugins' default" line | d6, d8 |
| `crates/oml-app` and `oml-server-lead` whoami; `docs/api/openapi.yaml:4788-4793` | no `shared` | d9; d10 test 16 |
| `sdks/python/src/oml_sdk/generated/models/whoami.py`, `sdks/typescript/src/generated/schema.ts` | generated from `openapi.yaml` | regenerate (CI: "generated SDK code is current with openapi.yaml") |
| `plugins/oml/.claude-plugin/plugin.json:13-18` | `url` default | d2 |
| `scripts/package_agent_artifacts.py:44-54, 127-128` | `DEFAULT_URL`, `ENDPOINT`; the Codex comment naming the public server | d7 |
| `scripts/check_plugin.py:13-14, 146-156, 202, 252-273` | docstring, `DEFAULT_URL`, the expected `url` option, `the_default_url_is_the_clis` | d10 test 2 |
| `scripts/check_agent_kit.py:2, 22-24, 54-65, 97, 215, 245-255` | docstring, `LITERAL_URL` per configuration, the default-server checks | d10 test 3 |
| `scripts/check_contract_sync.py:31, 120` | the host as an allowed literal | drop the exemption |
| `kit/README.md:37-48, 64, 117` | the public server as every configuration's URL | d7 |
| `plugins/oml/README.md:15, 27, 43, 57-58, 326` | the default and how to leave it | d2, d4 |
| `README.md:34-41, 53, 61` | quick start and table on the hosted service | Consequences, "Pages" |
| `docs/start/install.md:32`, `docs/start/index.md:21-23` | "The default server is …", sign-up on the public server | same |
| `docs/guides/claude-code.md:15, 31, 45, 49, 123, 126` | the public-server path as default | same |
| `docs/guides/other-harnesses.md:4, 39, 72`, `docs/llms.txt:15` | front matter and `llms.txt` line ("the public server by default"); "every file names the public server" | d7; `check_contract_sync.py` holds the two equal |
| `docs/guides/get-an-agent-key.md:13, 19`, `docs/guides/claude-ai-and-mobile.md:30-32` | the public server | same |
| `docs/help/troubleshooting-client.md:55` | "a `url` left unset is the public server" | d8 |
| `docs/use/sync.md:22`, `docs/use/chat-memory.md:32` | public-server consent | d9 |
| `docs/faq.md:21-23, 109`, `docs/reference/limits.md:28, 49`, `docs/engineering/sdks.md:17` | the public server by name | reword without it |
| `docs/product/evaluate.md:18`, `docs/product/is-oml-for-you.md:31` | "The public server removes that work" | reword without it here; the hosted pitch moves with the landing (#194) |
| `docs/cli/reference.md:838, 996, 1004` | generated from clap | regenerate |

`site/**`, `docs/legal/**` and `scripts/tests/test_site_build.py` also name the hosted service; they
leave the repository with the landing (#194), not with this record.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Keep `https://memory.doublegate.io` as the default** (ADR-0043 as accepted) | Against the operator's decision of 2026-11-11. Every release, and every fork built from it, would keep naming one operator's service, and a self-hoster who forgets one override sends their key to that operator (ADR-0043 d7's cost). |
| **A build-time default for hosted packages** (an `option_env!` constant and a plugin manifest written at release, empty in this repository's build) | This repository produces the released CLI, plugin and kit, so either its release carries the hosted value again or a second pipeline publishes a second `oml@open-memory-lab` with different behaviour under the same name. A user could not tell which build they hold, and every path would be tested twice. |
| **A well-known discovery URL or DNS lookup** (`/.well-known/oml`, an SRV record found from an email domain) | Discovery needs a name to start from. The key carries none, and an email domain needs a resolver service, which is a default server under another name. |
| **Encode the server in the key value** (`oml_<host>_…`) | Changes the key format, which encodes nothing and has a fixed prefix for secret scanners ([roles and keys](../architecture/roles-and-keys.md#key-format)). It puts a host name in every pasted key, and existing keys would still carry none. |
| **A key file that also holds its server** (two lines, or JSON) | The printed configure line reads the file with `jq --rawfile` into `api_key`, and existing key files hold the value alone. The issuing server already prints `--url` beside `--key-file` (d6). Reopen if a reader other than setup needs the pair in one file. |
| **Pair a `--key-file` key with `OML_URL` or the credentials file's `url`** | Mixes two sources. A stale `OML_URL` left from another server would receive the key, which is what ADR-0039 d15's pair rule exists to stop. On a terminal, the prompt asks instead. |
| **A placeholder that resolves** (`https://oml.example.com/mcp` in the kit) | `example.com` resolves to hosts IANA runs, so an unedited copy would send the key there. `.invalid` never resolves. |
| **A loopback default** (`http://localhost:8080`) | ADR-0040 rejected it: it sends the key to whatever listens on that port. |
| **Keep the consent keyed to a host list shipped in the CLI** | A list of hosts is the default by another name, and it goes stale when a server moves. |
| **Drop `oml sync`'s consent** | Loses ADR-0057 d6's disclosure to someone syncing a repository into a server whose admins are not theirs. |
| **Ask consent of every server** | Every self-hoster would confirm their own server once per clone, and a prompt that always appears stops being read. |
| **Treat a `whoami` without `shared` as shared** (the record's proposed reading) | The operator chose against it on 2026-11-11: every self-hoster on an older server would confirm their own server once per clone until they upgrade. The cost of the choice is in d9: an older shared server gets no disclosure until it upgrades and sets `server.shared`. |
