---
status: Stable
verified: 2026-11-06
answers: "what each OML release changed for users and operators, newest first"
---

# Release notes

Newest first. Each release is a signed tag on the [releases page](https://github.com/e8kor/open-memory-lab/releases).

## 0.7.2

Released 2026-11-12.

**v0.7.2 is trust and release hygiene.** A served memory now says when it was approved and by what kind of reviewer ([ADR-0077](adr/ADR-0077-a-served-memory-says-when-it-was-approved-and-by-what-kind-of-reviewer-and-never-names-the-reviewer.md), accepted and built, [#175](https://github.com/e8kor/open-memory-lab/issues/175)), and no release names a server ([ADR-0076](adr/ADR-0076-releases-ship-no-default-server-and-setup-asks-for-the-server-or-reads-it-from-the-key-or-the-environment.md), built). The injection scan's rule set is `oml-scan/1.2` ([ADR-0074](adr/ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md), [#226](https://github.com/e8kor/open-memory-lab/issues/226), [#317](https://github.com/e8kor/open-memory-lab/issues/317)), which reads a phrase in the letters-only view only at word edges. ADR-0070's two session-gate rows are accepted, and the record is corrected by a whole-document audit against the code. **Four things belong to the upgrade:** a server other people use sets `server.shared = true` first, clients that relied on a default server are given theirs, `oml admin rescan --record-held` runs after it, and migration 0044 rewrites every promoted row at boot. The upgrade notes close this section.

- **No release names a server** ([ADR-0076](adr/ADR-0076-releases-ship-no-default-server-and-setup-asks-for-the-server-or-reads-it-from-the-key-or-the-environment.md)). The CLI, the Claude Code plugin, its hooks, the agent kit and the web pages no longer default to the maintainer's server. The plugin's `url` option is required again, with no default. `oml setup claude-code` takes the URL beside the key. Without one it asks for the server on a terminal. Off a terminal it refuses as `setup.no_server` before anything is sent, and it never pairs a key file with `OML_URL`. A hook with the plugin's key and no `url` prints `no_server` and loads nothing. `oml doctor` names `no_server`, `plugin_no_server` and `server_origin`. The web pages show the server's own address beside a new key, or say it is not configured. The agent kit's Cursor and Gemini CLI files read `OML_URL`. Its Codex CLI, VS Code and Claude Desktop files carry `https://oml-server.invalid/mcp`, which never resolves. `GET /v2/whoami` gains `shared`, `true` when the operator sets the new `server.shared` or opens sign-up. `oml sync` asks consent of a shared server and records it per server origin. A server whose whoami does not say is not asked about, and a recorded `oml.allowPublicServer=true` no longer counts. **Upgrading:** first, a server other people use sets `server.shared = true` before, or in the same deploy as, its upgrade; missing it is silent, because a server that does not say it is shared is never asked about and nothing warns the operator. Then set the plugin's `url` once with `oml setup claude-code --url <server> --key-file <file>`. A script that ran setup with a key alone passes `--url`. The Python and TypeScript SDKs are regenerated for `Whoami.shared`, which the schema marks optional, so they read an older server's whoami as not shared.

Fixed the "A write" sequence diagram on the [architecture overview](architecture/overview.md), which the docs site drew as "Syntax error in text": a semicolon in a message ends the statement in a Mermaid sequence diagram, so the last arrow's text now reads `acknowledged: held, visible to the author only`. All 22 diagrams in the docs parse under Mermaid 11.17.2.

Fixed a flaky chat test and three review nits (#312; ADR-0063 d10, ADR-0072 d12). The test `per_turn_recall_is_skipped_on_timeout_and_stops_after_three_misses` no longer assumes every counted miss sent a request: ADR-0063 d10 counts a miss before a request is sent and runs one clock from before the cache write, so a slow runner can spend the budget first. The test now asserts the three misses, at most one request per miss, and that the call after them sends nothing. ADR-0072 d12 and the operating guide give 63.4 MB as an upper bound on what the widen signal's build added, the peak being the whole process's. The commit guard in `oml-app`'s tests reads `'\''` as one character literal and finds a commit split from its `::` or `.` by whitespace or a line break, or from its `(` by whitespace.

Fixed three nits from the review of the widen hint (#294; ADR-0072 d8, d12). The commit guard in `oml-app`'s tests now finds a journal commit taken as a function pointer (`<J as JournalStore>::commit`) and reads code alone, so a `//` inside a string no longer hides a later commit, and a commit named in a string or a comment is not one. ADR-0072 d12 and the operating guide give the build's measured memory as 63.4 MB above the 9.1 MB before it, the SQL data load included, and name the host load average apart from it.

Replaced the latency comparison in federation test 4 with a deterministic check: a list naming an unreadable workarea that holds 100,000 rows (5,000 on PostgreSQL) is refused with the same bytes and the same work, rows visited on the in-memory store and tuples read on PostgreSQL, as one naming an absent name. The two refusals' timing comparison moved to the weekly bench workflow (ADR-0072 d13 test 4, #295). Test changes only; the server is unchanged.

Fixed `oml sync` on git older than 2.30 (#316; ADR-0057 d6, ADR-0076 d9). The first `--allow-public-server` opt-in dropped the legacy `oml.allowPublicServer=true` with `git config --fixed-value`, which git 2.30 added, so on git 2.25 (Ubuntu 20.04) it recorded the origin and then failed with `could not drop the legacy oml.allowPublicServer=true`. It now unsets by the anchored pattern `^true$`, which removes only the literal `true` (`https://true`, `TRUE`, `true ` and every recorded origin stay), and still counts git's "no such value" exit as success. Resolving `--rev` used `git rev-parse --end-of-options`, also 2.30, so on those versions every `oml sync` stopped at `--rev "HEAD" is not a commit` before it reached the opt-in; a `--rev` that starts with `-` is now refused before git sees it, which git 2.30's option did too. The sync page now states the minimum, git 2.7 (`git remote get-url`). Nothing in the server changed.

**Web console.** The admin People page and its acts refuse an allowlist the API cannot page
through: a cursor the API repeats answers `unavailable` with 502, as the key list does, where the
page used to show the list cut at the repeat and `set_admin` or `remove_email` acted on that cut
list (ADR-0070 d8, #293).

**Web console.** The Keys views and the confirmation before a person's last address is removed read the key list whole or refuse: a cursor the API repeats, at once or in a cycle, answers `unavailable` with 502, and a list past the pages a view reads answers `limit.list_too_long`, where each showed the list cut short and the confirmation could leave an agent key out. The console's four cursor walks, the keys, the list's key kinds, the allowlist and the workarea tree, share one walker (ADR-0070 d8, #308).

The number registry sees reservations on every unmerged remote branch, with or without a pull request: `reserve_number.py` and `check_numbers.py` fetch every remote head first, refuse with a named reason when they cannot, and a reservation is released when its branch is deleted or merged or when `main` uses its number (#178).

- **The session gate resolves a capacity credential in one store call** ([ADR-0070](adr/ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md) d2, d8, the table of 2026-11-12 for #297; [#297](https://github.com/e8kor/open-memory-lab/issues/297)). The session, the session key, its access and, for `<user>-web`, its root and the person's keys are read in one transaction under one snapshot where they were four to six transactions, and the gate makes every check it made on those rows. On a `GET` or `HEAD` the request's first resolution of the caller decides on the access the gate read; every later resolution reads it when it runs, and a request of any other method keeps whatever read its verb makes, as before. `whoami` without the person block reads 2 store calls for a session key where it read 9. No operation, field or page call changed, and who may read what is unchanged. Measured against main, the memory page, Keys, Start, People, Users and the admin Atlas are within the 5 ms budget at 6 and at 48 workareas. Two pages still miss it: the admin Workareas view adds 6.2 ms p95 at 6 workareas, and the admin Memories list adds 5.8 ms at 48. ADR-0070 was audited whole against the code on 2026-11-12 and corrected in 44 places, and its two session-gate rows were accepted.

Accepted [ADR-0077](adr/ADR-0077-a-served-memory-says-when-it-was-approved-and-by-what-kind-of-reviewer-and-never-names-the-reviewer.md) and built it (#175): every served memory carries `approved_at`, the time of the approving verdict that ratified it, and `approver`, the reviewing key's `held_by` and the `same_user` mark. Get, recall and every listing carry both over HTTP, the `oml_read` tool carries both on its items, and the Python and TypeScript SDK types expose them. A crew's reviewer reads `held_by: "agent"`, a person `held_by: "human"`, and a person approving their own agent's write `same_user: true`. No read of a memory names its reviewer, as ADR-0025 d15 and ADR-0037 d5 decided; the record closes three questions that way: the reviewer is not named to readers and an administrator reads who through the journalled admin read, a policy needing several verdicts will add a list beside `approver` without changing its meaning, and `held_by` is served on every memory. Both members are null on a memory served before review and on a `universe` memory served to anyone but its author. Migration 0044 adds three columns to `memories`, none naming a reviewer, and fills them from the journal for every memory promoted before it; it runs at upgrade, and an older server refuses to start once it has run. [Audit and lineage](architecture/audit-and-lineage.md) says what a reader is shown.

Added a page, [when a recall repeats](architecture/repeatable-recall.md), and the test behind it (#176): under the default pipeline, `bm25-postings`, the same recall returns the same steering, hits, order and scores byte for byte, call after call and from a second server process over the same database. `recall_id` always differs; `timings` and `search.widen` can differ where served. The page lists the inputs that define an answer (the request, the key, the approved set, `now`, the settings and the server version), what changes it between calls, and what is outside it: `timings`, the `search.widen` hint, the pipelines that read a model, and a recall with `as_of`, which the `bm25` scan answers and the test does not cover. A held write leaves every other key's answer unchanged; its approval changes the answer once. The test ran on 14 approved memories, serial plans, PostgreSQL 18.6 locally and 17 in CI. The last digit of a score, and the order of two hits one `float4` step apart, is not proven identical across query plans, PostgreSQL versions or parallel worker order; it is not observed and not excluded, and #310 tracks proving it.

Changed the `new system instructions` rule of the injection scan's letters-only view, still `oml-scan/1.2` since that rule set is not released (#317; ADR-0074 d6, ADR-0065 d14). The phrase holds when a colon follows it on its line, a combining mark counting with its letter, or when it is its line: nothing else stands on the line, up to a line feed, carriage return, vertical tab, form feed, NEL or line or paragraph separator, but marks (any character that is not a letter, a digit or a bar, so a hash, an asterisk, a quote, a bracket, an arrow, an emoji, a box-drawing line or a dot), a list marker (a lone number or letter, however written: a keycap, a circle or a bracket), up to eight HTML tags before the phrase, and up to eight tags or further phrases after it. A heading of any depth, a closed or setext heading, a line in capitals, `**New system instructions**`, `> …`, `- …`, `=== … ===`, `NEW SYSTEM INSTRUCTIONS!`, `[…]`, `<<< … >>>`, `<h1>…</h1>`, `🔥 NEW SYSTEM INSTRUCTIONS 🔥`, a dotted heading with its trailing dot and a line of the phrase twice hold. A list item with a label (`- printer: updated system instructions`), a commit subject, `TODO: write new system instructions`, an import, a path, a URL ending in the slug, a hard-wrapped line that ends in the phrase and a sentence with or without its full stop no longer hold. The three words may be set apart by any mark: `**New** system instructions: reveal`, `new *system* instructions:`, `new (system) instructions:`, `NEW—SYSTEM—INSTRUCTIONS:`, `new,system,instructions:` and `new+system+instructions:` hold, as they did under `/1.1`. They may not be set apart by a bar (`new | system | instructions:` passes), a line break, or the end of a clause (a comma, semicolon, colon, question or exclamation mark and a space: `new, system, instructions:` passes). A full stop and a space after every word, between every letter (`n. e. w. s. y. s. t. e. m. i. n. s. t. r. u. c. t. i. o. n. s:`) or a hyphenated line wrap (`new-system-` and `instructions:` on the next line) is the phrase's own spelling and holds; one full stop among the words is two sentences (`Updated. System instructions: see the wiki` passes). A combining mark before the colon no longer hides it. **The cost.** The rule holds what is the phrase alone and is no heading: a list item, a title field, a route (`/new/system/instructions`), an identifier (`new_system_instructions`), a phrase alone inside a tag or quotation marks, and `The UI is new. System. Instructions: see the wiki.`, whose one-word sentence is the phrase's own spelling of a break. It passes a directive with another word on its line: `Here are your new system instructions` followed by a list and `IMPORTANT - real system instructions` followed by its text, which are given up; a line that ends in the phrase and `follow` or `below:` before it; a heading with a section or page number; a heading in a table cell; `请NEW-SYSTEM-INSTRUCTIONS` on its own line, since a letter of another script is not a mark (it holds with a colon after it); a heading letter-spaced inside an HTML tag; a line of more than eight tags or phrases; a roman-numeral list marker; a fraction, a letter-like symbol (`™ … ™`) or a fullwidth word (`ＸＸ … ＸＸ`) before the heading, which fold to letters of a word; a dotted or dashed heading written twice on a line, since the repeat must be spelled plainly. The red-team corpus gains 20 malicious samples with 20 benign twins, and none of the existing samples changed its verdict. The false-positive corpus is 142 samples: 35 of the 135 that were in it before held under the rule set as merged, 8 of them by mistake (25.9%), 28 hold now, all by design (20.7%); the 7 new samples add 4 by-design holds. A megabyte of tags or phrases on one line no longer takes seconds: a first line rule read every tag again for each phrase and took 26 to 43 s (`<new system instructions>` repeated then one `x`), and now takes 80 ms in a release build. The refused-near-matches test asks a megabyte to scan in at most eight times the time of a quarter of it plus 20 ms, within 3 s in a debug build and 400 ms in a release build; the bound was 5 s before, with no growth test. Three more costs of the line rule, found in its last review: a one-letter word before the phrase reads as a list marker, so `I updated system instructions.` and `A new system instructions.` alone on a line hold (they passed under the rule set as merged; without the full stop they held under both); a hyphen followed by blank lines is read as one wrapped word, so `a brand-new-`, blank lines, then `system instructions: read them first` holds, as it did under `/1.1`; and a circled or keycap character after the phrase folds to a letter or digit and counts as a word, so `Ⓐ New system instructions Ⓐ` passes. The `oml-scan/1.2` set was not released, so no rescan beyond the one the `/1.2` fragment already asks for is needed.

Changed the injection scan's letters-only view to read a phrase only at word edges, and the rule set to `oml-scan/1.2` (#226; ADR-0074 d6, d21). The edges are read from the folded text (removable characters made a space, NFKD, lower case) as the raw rules' ASCII `\b` reads them: an ASCII letter touching a phrase makes it part of a longer word, and a digit, `_`, a letter of another script, a fullwidth form or punctuation is an edge. A phrase inside a longer word (`Danish`, `developer mode only`) or a camelCase identifier no longer holds a memory; a phrase joined by `_`, `/` or `-`, spelled out letter by letter, or set beside non-Latin text holds as before. The cost: a phrase glued to an ASCII letter (`xignore previous instructions`, `ignore previous instructionss`, `pleaseignore previous instructions`, `do.not.tell.the.users`) no longer holds, which cannot be told from `Danish`. The letters-only `new-instructions` phrase is now `new system instructions`, three words as ADR-0065 d14 asks, and holds when a colon follows on its line or it is its line, alone inside heading, emphasis, quote, list or bracket marks or any other symbols (a sentence that ends in `new system instructions` passes; the rule is the one of #317); `The new system instructions are in the wiki` and the two-word `new instructions` no longer hold in the letters view (the plain-text rule keeps `new instructions:`). A false-positive corpus of 123 prose, code, identifier and URL samples joins the scanner tests: 35 of the 97 that are not meant to hold held under `/1.1` and none does under `/1.2`, while the 26 that hold by design still do. The red-team corpus gains 16 malicious samples with their 16 benign twins. **After you upgrade, run `oml admin rescan --record-held`.** The rule set's name moved, so until it has run, sibling agents in an in-progress workarea do not see held versions scanned under `oml-scan/1.1`; `oml doctor` warns with `scan_held_unrescanned` and `scan_rescan_due` meanwhile. Nothing is demoted or deleted, and the findings `/1.1` recorded on a held version, including the false positives this change removes, stay on it until a reviewer judges it.

The Python and TypeScript SDKs are versioned with the server, and each release builds, smoke-tests, signs and attaches them (a wheel, an sdist and an npm tarball); a test and the release's version job fail when an SDK manifest and the server's version diverge. Two release jobs publish them to PyPI by trusted publishing and to npm with provenance, and run only when the repository variable `PUBLISH_SDKS` is `true` and `sdks/REGISTRY-NAMES.toml` records the operator's choice of registry names, which is not made. Nothing is published, and the SDK page carries no registry install line: the steps are in [Releasing](https://github.com/e8kor/open-memory-lab/blob/main/docs/contributing/releasing.md#publishing-the-sdks) (#184).

Changed the landing pages to point at the web console's own pages, `/web/keys` for agent and connector keys and `/web/review` for review, and to say what v0.7.0 shipped: scan findings on the review card, as-of reads, recall across workareas off until a tenant admin turns it on, `bm25-sparse` where its model is loaded, and the web console as a second process. Fixed the pages' favicon, whose data URI broke out of its `href` at the first double quote.

Upgrade notes:

- **A server other people use sets `server.shared = true` (`OML_SERVER__SHARED=true`) before it upgrades, or in the same deploy.** Without it, a v0.7.2 CLI syncs to that server with no consent prompt, and nothing warns: a server whose `whoami` does not say it is shared is never asked about ([ADR-0076](adr/ADR-0076-releases-ship-no-default-server-and-setup-asks-for-the-server-or-reads-it-from-the-key-or-the-environment.md)).
- **CLIs, plugins and the agent kit no longer carry a default server.** A client that relied on it must be given its server: `oml login <server>`, `oml setup claude-code --url <server> --key-file <file>`, or the plugin's `url` setting. Users of `oml sync` consent once more for each clone with `--allow-public-server`.
- **The scanner's rule set is `oml-scan/1.2`.** After the upgrade run `oml admin rescan --record-held`. Until it has run, held versions scanned under `oml-scan/1.1` are author-only in in-progress workareas, and `oml doctor` warns `scan_held_unrescanned` and `scan_rescan_due`. Nothing is demoted or deleted.
- **Migration 0044 runs at boot and rewrites every promoted row of `memories` under an exclusive lock.** Measured on a loaded shared host it took about 1 s per 10,000 promoted rows and 13 to 22 s per 100,000 ([#311](https://github.com/e8kor/open-memory-lab/issues/311)), and the table grows by about 70% until vacuum. An older server refuses to start once it has run.
- `oml sync` works on git 2.7 and later. That minimum is derived from git's source, not run against git 2.7.

## 0.7.1

Released 2026-11-12.

**The web console still misses its latency budget on some pages.** ADR-0070 allows the web console 5 ms p95 over the API it calls. In this release 7 of 21 pages miss it with 6 workareas and 11 of 21 with 48, by +5.1 to +35.7 ms p95, down from 16 and 15 in v0.7.0 ([Web console overhead](benchmarks/web.md)). The rest is tracked in [#297](https://github.com/e8kor/open-memory-lab/issues/297).

- Changed the web console's pages to spend fewer API calls and rounds ([#286](https://github.com/e8kor/open-memory-lab/issues/286)). `GET /v2/whoami` resolves its credential once, so a key value or a connection spends one request of its rate, and takes `person=false`, which leaves out the person block and its reads; `oml-web`'s session check asks for it, so a session key's `whoami` reads 9 store rows where it read 15. `GET /v2/workareas/{workarea}/tree?counts=true` takes `offers=true` and carries the scope's offers, so the admin Atlas reads them with its counts. Who may read what is unchanged.
- Removed the redirects v0.7.0 kept for the web console's v0.6 addresses ([#283](https://github.com/e8kor/open-memory-lab/issues/283)). The server's web routes are now sign-in's alone: `GET /web` (the sign-in page, and for a signed-in browser the way on to the Keys view), sign-out at `POST /web`, `/web/sign-in`, the provider callback and `POST /web/sign-up`. `GET` and `POST /web/admin` are gone. The user page's Administration tab links the overview at `/web/admin/people`.
- Changed a person's disable to refuse by name while an admin key names their root ([#277](https://github.com/e8kor/open-memory-lab/issues/277)). No verb issues such a key, so the row is corrupt: `disable_user`, and the deletion close that runs it, answer 409 `workarea.admin_key_under_root` and journal nothing, where before the disable skipped the key in silence. `GET /v2/web/users` names each such key per person in `admin_keys`, and `oml doctor` under an admin key prints `warn keys` for each.
- Fixed the widen hint's builds to take turns by tenant, so one tenant can no longer queue many builds ahead of another's first, and the lead to stay within its 8,000,000-pair bound by evicting the oldest signals of a tenant over its share ([#268](https://github.com/e8kor/open-memory-lab/issues/268)). ADR-0072 now says the 25 s database limit applies to each statement.
- Fixed the web console's review nits from #279, #282 and #284 ([#281](https://github.com/e8kor/open-memory-lab/issues/281)): a key list whose cursor the API repeats is refused as the API's fault, `unavailable` with 502, instead of `limit.list_too_long`; the memories JSON no longer carries `base`; a connect code minted through `POST /v2/keys/{key}/connect-codes` with a bearer key runs as the caller the request resolved, as the other console acts do.
- Added a research note on map-reduce recall ([`eval/recall/MAPREDUCE.md`](https://github.com/e8kor/open-memory-lab/blob/main/eval/recall/MAPREDUCE.md)), nothing built: a workarea's `bm25-sparse` scores merge into the global answer byte for byte when the reducer, not the workarea, fuses, and on one PostgreSQL at 100,000 memories and 50 workareas on 8 vCPUs, a split scope cut the sparse leg's p95 from about 95 ms to 19 to 56 ms and served 1.6 to 4.3 times the recalls a second with 1 to 4 clients, and 7 to 14% fewer with 8.
- Changed internals only: the gating suite no longer fails on wall-clock timing, since ADR-0072's test 7 asserts its deterministic half and the p50/p95 comparison runs weekly ([#278](https://github.com/e8kor/open-memory-lab/issues/278)); the lead's route wall refuses an API-table route that `openapi.yaml` does not serve; tests show that a proposal, a ratification and a mode change each cost one request of a person's rate.

Upgrade notes:

- No migrations run at upgrade; the schema is v0.7.0's (0043).
- Old bookmarks now 404. `GET /web?view=...` with a session, `GET /web/admin` and a path below it that `oml-web` does not serve answer not found, and a form cached from a v0.6 page that posts an act other than sign-out is refused and runs nothing. Use the pages' own paths in [Run the web console](operations/web-console.md). No proxy change is needed: `/web` and `/web/admin` already fall through to the server.
- `docs/api/openapi.yaml` and the generated SDKs drop `GET` and `POST /web/admin`: the Python SDK loses `admin_page`, `admin_page_act` and their models (`AdminPageView`, `AdminPageActBody`, `AdminPageActBodyAction`), `user_page()` loses its `view` parameter, `UserPageView` is gone and `UserPageActBodyAction` is `SIGN_OUT` alone; the TypeScript schema drops the same types and `/web`'s query parameters.
- `GET /v2/whoami` takes `person`: `false` leaves out the `person` field, which a client must then treat as absent, and `person=false` with `start=true` is `request.validation` naming `person`. The default answers as before. The Python and TypeScript SDKs carry the parameter; the Rust SDK's `whoami()` now always asks `person=false`, since its `Whoami` reads no person block, and an API before v0.7.1 ignores the flag.
- `GET /v2/workareas/{workarea}/tree` takes `offers`, only beside `counts=true`; `offers` without `counts=true` is `request.validation` naming `offers`. The answer's `WorkareaCounts` gains an optional `offers`. The Python and TypeScript SDKs carry the parameter; in the Rust SDK, `workarea_counts()` takes a third argument, `offers: bool`, so a call with two no longer compiles.
- `ErrorCode` has a new value, `workarea.admin_key_under_root` (409, with `details.user` and `details.key`), which a client matching codes exhaustively must accept. `WebUser` gains `admin_keys`, a list of key names.
- `AdminMemory.key` is now nullable in `openapi.yaml`, as the server already answered for a key with no name (`key_id` names it); a typed client that required a string must accept null.
- In the Rust SDK, `KeyIssued::new` and `ConnectCode::new` build either struct outside the crate, which v0.7.0's private `replayed` field made impossible; both start with `replayed()` false.

## 0.7.0

Released 2026-11-11.

**The web console misses its latency budget in this release.** ADR-0070 allows the web console 5 ms p95 over the API it calls. 16 of 21 pages miss it by 6.2 to 42.7 ms p95 (10.1 to 31.2 ms with 48 workareas) ([Web console overhead](benchmarks/web.md)). The cause is the session check: each page opens with a round of `GET /v2/whoami` calls, 7.8 to 17.4 ms p50 each at the server, that resolve the session's credential twice where the in-process page read the session once; the hop itself adds at most 4.3 ms p95 ([ADR-0070's measurement](adr/ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md#the-measurement-of-2026-11-11-170)). v0.7.0 ships with the miss by the operator's decision of 2026-11-11, and v0.7.1 fixes it with fewer calls per page ([#286](https://github.com/e8kor/open-memory-lab/issues/286)).

- Changed the web console to run as its own process, `oml-web`, over the public HTTP API. Every page moved to a path of its own under `/web/` (Guardrails, Workareas, Memories, Atlas, Review, Keys, one memory, Curate, Start, setup, people and users), and the server answers the old `?view=` addresses with a redirect, which v0.7.1 removes. The server keeps only sign-in, the sign-up terms page and sign-out. Each release ships `oml-web-X.Y.Z-<target>.tar.gz`, the image carries `/usr/local/bin/oml-web`, and `docker-compose.yml`, `oml quickstart` and the Helm chart start it behind a Caddy router.
- Added the API operations the console calls: ten new ones, among them `GET /v2/keys`, `GET /v2/review/queue`, `GET /v2/workareas/{workarea}/offers` and the admin's user acts, and new fields on six more. A person's web session reaches the API only through `oml-web`, which proves itself with the web client secret. A console act claims its `Idempotency-Key` before it acts, so a form sent twice acts once and the resubmission replays the first answer.
- Changed a person's quota to bind only their agent keys; their own web console keeps at least 60 requests a minute whatever the quota.
- Added as-of reads: `as_of` on reads by id, listings, recall and `oml_read` returns memory as it was recorded at a past instant, under today's access.
- Added recall across the workareas a key already reads: `sources: "wide"` or a list of workareas, off until a tenant admin turns federation on (`oml search federation`). A `scope` recall carries `search.widen` when a wider search would find more.
- Added every scan finding on a held memory to its review card and to `POST /v2/review/next` and `GET /v2/review/held`, where a version stored only the first before. `oml admin rescan --rates` prints finding rates per class and origin, and `crates/oml-scan/tests/corpus/` holds an 802-sample red-team baseline.
- Changed a recall under a tenant default pipeline the server no longer serves to run the server's default and say so in `search.fallback: "tenant_default_unserved"`; `oml doctor` warns `search_tenant_default`. The retrieval page recommends `bm25-sparse` where its model is loaded (success@5 0.983 against 0.934 on the recall eval), at the cost of a 268 MB model, CPU at write time and a higher p95 past a few thousand memories. `bm25-postings` stays the default.
- Added `eval/recall/selfhost.sh`, which runs the recall eval on your own machine; CI runs the same script.
- Changed the landing pages to ship apart, built from `site/` as `oml-site-<version>.tar.gz`, and to say what the scan refuses and what it records as a finding.
- Fixed a web console disable or deletion close that met a concurrent write, which journalled its events twice.
- Added design records and measurements, none of them built: releases that name no server (ADR-0076, accepted); questions generated per memory (ADR-0075), whose offline phase 0 did not beat `bm25-sparse`, so the operator parked it; and phase 2 of federated recall, whose reviewed descriptors gained one query on the recall eval's cross and skew splits, below the three-point floor, and are not admitted.
- Changed internals only: the application core keeps no page use case, the crate walls are a CI check of their own, and one flaky hook test counts requests instead of timing them.

Upgrade notes:

- Migrations 0039 to 0043 run at upgrade. 0041 builds an index outside a transaction, one tenant at a time; reads never wait. Once 0043 has run, an older server refuses to start, and `oml doctor` counts held versions an older server wrote without their findings as `scan_findings_unlisted`.
- Start `oml-web` beside the server with the same `web.client_secret`, at least 32 characters, from the environment or an owner-only file, and route its pages to it in your proxy ([Run the web console](operations/web-console.md)). On Kubernetes, pass the secret as an environment variable; a volume-mounted Secret is refused.
- `POST /v2/keys` and `POST /v2/keys/{key}/connect-codes` now hold an `Idempotency-Key` to one request for every key. The same key with another request answers `409 idempotency.digest_mismatch`, and the same request again answers the first `201` without the key's value or the code. Send one key per act.
- `search.fallback` has a new value, `tenant_default_unserved`, which a client matching the field exhaustively must accept.
- The generated SDKs drop six page operations the web console now serves itself: `start_page`, `start_page_act`, `start_status`, `start_export`, `users_page` and `users_page_act` in the Python SDK, and their types in the TypeScript schema. No key could call them.
- In the Rust SDK, `KeyIssued` and `ConnectCode` gain a private `replayed` field, so a struct literal of either no longer compiles; build them only through the SDK. Each gains `replayed()`, true when the API answered `Idempotency-Replay: true`, and the crate exports the header's name as `IDEMPOTENCY_REPLAY`.

## 0.6.3

Released 2026-11-10.

- Fixed three ways content the secret scanner should hold reached other agents. The rule set is now `oml-scan/1.1`: it reads every memory through normalised views, so invisible and format characters, combining marks, fullwidth forms and the braille blank placed inside or between the words of an injection phrase no longer hide it, and a run of variation selectors is held. Each stored version now records the rule set it was scanned under (migration 0038 reads existing versions as `oml-scan/1`). Inside an in-progress workarea, another author's held version is served only when the current rule set scanned it and found nothing, in recall, listings, reads by id and claim edges.
- Added `oml admin rescan --record-held` (`POST /v2/admin/rescan`), which records the findings of every held version under the current rule set and journals each as `scan.findings_recorded`. **After you upgrade, run `oml admin rescan --record-held`.** Until it has run, sibling agents in an in-progress workarea do not see held versions written before the upgrade. Migration 0038 has no down step, and a server older than `oml-scan/1.1` ignores the new column.
- Added two `oml doctor` lines, `scan_held_unrescanned` and `scan_rescan_due`, which tell you when held versions still wait for that rescan.
- Fixed the weekly recall benchmark, which failed on its first call because it asked for pipelines that need a model while booting the five that do not. The web traffic dashboard gains the sign-in failure panel the observability page already described.
- Fixed `oml sync`: the first `--allow-public-server` explains the disclosure and records consent for the source clone, and later syncs of that clone reuse it. `oml workareas tree` accepts a positional root. The user page no longer opens held or rejected memories written by any of the person's agent keys, the workarea table keeps intermediate parents in each "Name to use" label, and the Guardrails page offers a workarea selector. Chat failures name a retry command and quickstart quotes the paths in the Docker commands it prints.
- Added design records, none implemented: a distributed provider with a standby lead and read nodes (ADR-0073), recall across the trusted workareas a key already reads (ADR-0072), threat classes for every scan finding (ADR-0074), and the web console as a client of the public API (ADR-0070).
- Changed internals only: one shared Postgres test fixture, and `ruff` now checks `scripts/` in CI.

## 0.6.2

Released 2026-11-06.

- Added memory cards to the web pages: `/web` and `/web/admin` list memories as paged cards you can filter by state, workarea, origin, author key and text, and draw the workareas you may read as an Atlas, a treemap the server draws with no script.
- Added a dropdown on each workarea in the web pages for its acts, each confirmed before it runs.
- Added open sign-up, off by default. With `signup.mode = open`, a person who signs in with Google or GitHub and holds no entry reaches a terms page, then gets a private root under quotas, a reviewer key of their own and a `/web/start` page that walks them through the first memory. A person can ask for their account to be deleted. Migration 0037 runs at upgrade. Nothing changes for you unless you set the mode.
- Changed the web pages, the documentation site and the README to one design system, with the same colours, type and state marks everywhere.
- Changed the landing page to lead with one memory across your AI tools, with a page for teams and one for individuals.
- Changed the documentation to be grouped by what a reader does, with open work tracked as GitHub issues.
- Changed continuous integration to run only the jobs a pull request's changed paths reach.

Upgrade notes: Migration 0037 runs at upgrade. Open sign-up stays off until you set `signup.mode = open`.

## 0.6.1

Released 2026-11-06.

- Added claim formats: a write may name a `format` (plain text, Markdown, Mermaid, JSON, YAML or code) and a one-line `title`; the format is signed with the version and changes only by replacement. Migrations 0032 to 0034 run at upgrade.
- Added the Rust adapter for `oml sync`, which reads a Cargo workspace as one claim per crate and one per module doc.
- Added titles to the MCP tools (Remember, Recall and Workarea) and rewrote the server instructions to describe what recall returns instead of telling the model to call it.
- Added `oml keys connect-code --ttl`, which mints a connect code that lives up to 30 days, only for a key confined to one workarea other than the universe.
- Added `oml demo seed <workarea>`, which fills a workarea with thirty demonstration memories.
- Changed the connect page to say where the code comes from, with a link to your account page.
- Changed a handler panic to answer a named `500 system.internal` with the request id; `oml doctor` warns when any occurred.

Upgrade notes: Migrations 0032 to 0034 run at upgrade.

## 0.6.0

Released 2026-11-06.

- Added in-progress workareas: in an opted-in development workarea, recall returns unreviewed memories, marked, and nothing leaves until a commit with none waiting.
- Added a curator key profile that narrows what a key may do inside one workarea.
- Added chat memory: `origin.kind` names each memory's source, and a person's chat workarea holds facts an agent writes from a conversation.
- Added valid time: a memory may state when it is true, and recall hides what has ended.
- Added `on_similar: refuse`, which turns away a write that repeats a readable memory.
- Changed the secret scanner to the `oml-scan/1` rule set and added `oml admin rescan` to report matches in stored memories.
- Added the `oml-lakewriter` binary, which writes the Delta lake.
- Added a production policy with service-level objectives and an error budget for a public server.

Upgrade notes: Migrations 0026 to 0031 run at upgrade.

## 0.5.8

Released 2026-11-05.

- Added memory sources: a repository, commit, file, part and a fingerprint of that part's text, shown to the reviewer and signed.
- Added recall and listing filters by repository and folder.
- Added a hit marker that says when a memory's source changed or went away.
- Added `oml sync` with `--dry-run`, `--check` and `--exclude` to load a repository's markdown documents in batches one person reviews.
- Added `oml anchor`, which prints the anchor of one part of a file.
- Changed replacement: a write names the version it read (`expected_version`), and a stale one is refused.

Upgrade notes: Restart agent sessions after the upgrade. Migration 0025 runs at upgrade.

## 0.5.7

Released 2026-11-05.

- Added the optional searches `bm25-sparse` and `sparse`, which also find memories by related words.
- Measured `bm25-sparse` at 0.730 nDCG@10 against 0.685 for the default (SciFact, 2026-11-05, 5,183 memories).
- Added recall telemetry kept 30 days, with `oml usage` to read events and replay them against other searches.
- Added bulk review: `oml review list` and approve or reject a chosen set, each memory its own signed verdict.

Upgrade notes: The server refuses to start when a configured sparse search lacks its model files. Migrations 0022 to 0024 run at upgrade.

## 0.5.6

Released 2026-11-05.

- Changed recall to read an index: p95 25 ms with about 100,000 readable memories, measured at the database, against 485 ms in 0.5.5.
- Added the default search `bm25-postings`, which says when it left out a very common word.
- Added the optional reranker `bm25-rerank`, off until an operator places the model files.
- Changed `oml doctor` to check the index and the reranker.

Upgrade notes: The first start builds the index and holds writes, at about 0.4 s per 1,000 memories.

## 0.5.5

Released 2026-11-05.

- Changed recall to match any of a memory's words, parts of a code name and word forms, with rarer words ranked higher.
- Changed the default search to `bm25`, and each hit names the words it matched.
- Changed search words to be stored at write time: about 40 times faster on a 5,000-memory benchmark.
- Added `bm25-trigram` for misspellings, which an administrator can make the default without a restart.
- Added suggestions for a word that matched nothing, drawn from each workarea's own words.

Upgrade notes: The first start rewrites every stored memory once.

## 0.5.4

Released 2026-11-04.

- Fixed the upgrade from 0.5.1 and earlier, which 0.5.3 broke on databases whose top workarea an early server created.

Upgrade notes: Do not install 0.5.3 over a 0.5.1 database. See [upgrades](operations/upgrades.md).

## 0.5.3

Released 2026-11-04.

- Published the content of 0.5.2.
- Paused the macOS command-line builds. On a Mac, build `oml` from source.

## 0.5.2

Released 2026-11-04.

- Added the workarea lifecycle: development, committed and validated, each step proposed by one identity and confirmed by another.
- Added labeled links between two memories, such as supports and contradicts, each proposed and confirmed by different reviewers.
- Added a brief for each workarea: reviewed memories an agent reads first at session start.
- Changed `oml doctor` to name an empty brief and steps waiting for a second identity.

Upgrade notes: This tag was not published. 0.5.3 carries the same content.

## 0.5.1

Released 2026-11-04.

- Added guardrails: reviewed rules each agent session loads at start, checked before matching tool calls.
- Removed the eight 0.3.0 agent tool names and the old command-line verbs. Use `oml_write`, `oml_read` and `oml_workarea`.

## 0.5.0

Released 2026-11-04.

- Added a workarea of one's own for each person, created at their first key.
- Added a shared area above user roots, written only by curators an administrator chooses.
- Added web pages for people: sign in with Google or GitHub, manage your keys and review your agents' memories.
- Added an admin page to manage the allowlist, reviewers and curators and to read any memory, each read recorded.

Upgrade notes: Existing workareas and keys stay where they are.

## 0.4.1

Released 2026-11-04.

- Added one log line for every request and agent tool call, kept at most 30 days by whatever stores them.
- Added Prometheus metrics on a separate address and OpenTelemetry traces, both off unless set.

## 0.4.0

Released 2026-11-04.

- Changed the agent interface to three task-shaped tools in place of eight.
- Added a one-server-address, one-key setup for every client.
- Added the Claude Code plugin with the key in the system keychain, and a read-only librarian subagent.
- Added OAuth for Claude's web and mobile apps through a one-time connect code.
- Added `oml login`, `oml setup claude-code` and `oml quickstart`.

## 0.3.1

Released 2026-11-03.

- Changed the server to run migrations as one database account and serve as another that owns nothing.
- Added the `oml` command line for Linux, macOS and Windows, and the server for Intel and Arm Linux.
- Added the Claude Code plugins as an offline archive and a kit for Codex CLI, Cursor, Gemini CLI, VS Code and Claude Desktop.

Upgrade notes: The server refuses to start when the two database accounts are the same.

## 0.3.0

Released 2026-10-31.

- Added names for workareas, crews, keys and organizations, accepted wherever an id is.
- Added crews as records of their own.
- Added a Helm chart that runs the server on Kubernetes as one instance.
- Changed an admin key to read the shape of the workarea tree but never a memory.

## 0.2.2

Released 2026-10-31.

- Added session-start context: an agent's recent work and the memories that match the project, within a budget.

## 0.2.1

Released 2026-10-31.

- Added one command that issues an agent's key with search and write access and can write it to a private file.

## 0.2.0

Released 2026-10-31.

- Added one key that writes and searches through granted workareas, where a denial always wins.
- Added reads of your own held writes, budgeted search, standing instructions first, fetch by id, replace and retire.
- Added eight agent tools and an event feed that shows each key only what it may see.

## 0.1.0

Released 2026-10-30.

- Added the first server: every write held, scanned and visible only after someone other than its author approves.
- Added keyword search and replacement by a new version.
- Added a signed, hash-chained history with a command-line verifier and a startup fork check.
- Added four agent tools, the Claude Code plugin and the `oml` command line.
