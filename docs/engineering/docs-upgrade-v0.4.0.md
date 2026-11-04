<!--
id: engineering/docs-upgrade-v0.4.0
tier: engineering
status: plan for review · not started
answers: which pages the v0.4.0 documentation upgrade rewrites, adds or retires; for each, what changes, which record or file is its source of truth, and whether it waits for the API rewrite, the installer verbs or OAuth; the simplified scenarios that replace the use-case catalogue; the order of work and the checks that close each step
-->

# Documentation upgrade for v0.4.0: a page-by-page plan

**Status:** plan for review · not started

v0.4.0 changes how a person starts with OML and what an agent calls. This page lists every page
that change reaches and says, for each one, what changes, where its truth comes from, and what it
waits for. The pages follow the rules of [ADR-0018](../adr/ADR-0018-the-documentation-set.md). A
page that would describe software not yet built keeps its "decided, not built" marker until the
software exists.

## What the upgrade rests on

| Handle | Record | Where it is today | What it gives the docs |
|---|---|---|---|
| **R** | the API rewrite, proposed ADR-0039 | branch `feat/api-rewrite` | three MCP tools, `oml_write`, `oml_read` and `oml_workarea`; one `OML_URL` and `OML_API_KEY` pair on every client (d15); `oml login` and `oml doctor`; `GET /v2/whoami` |
| **P** | plugin configuration in Claude Code, proposed ADR-0040 | branch `feat/api-rewrite` | `userConfig` with `url` and a sensitive `api_key`; `claude plugin configure --values-stdin`; Claude Code 2.1.285 or later |
| **Q** | the quick start, proposed [ADR-0041](../adr/ADR-0041-a-quick-start-in-one-command-per-role.md) | `main` once merged | `install.sh` and `install.ps1` (built); `oml setup claude-code` and `oml quickstart` (not built); the time targets, FR-68 |
| **O** | OAuth through a connect code, proposed ADR-0031 | branch `feat/oauth-connect` | claude.ai and mobile reach OML without pasting a key |
| **U** | the use cases and their call map | PR #26, branch `feat/use-cases` | `docs/product/use-cases.md` and `docs/engineering/use-case-call-map.md`, which this upgrade rewrites |

The **Waits for** column below uses these letters. **—** means the page can change now.

Every page in the upgrade follows four rules.

- **One pair.** The only client settings named are `OML_URL` and `OML_API_KEY`. `OML_CLI_SERVER`,
  `OML_CLI_KEY` and `oml context` leave every page once R is built.
- **Three tools.** Agent-facing pages name `oml_write`, `oml_read` and `oml_workarea`. The twelve
  catalogue entries of today appear only in the migration table of the upgrades page.
- **Held is said up front.** Every page that shows a first write also says, in the same
  paragraph, that the write is `held` and readable by its author at once, but by others only after
  a reviewer approves it.
- **Targets stay targets.** A time is written as ADR-0041 d10's target with FR-68, never as a
  measurement, until the measurement exists.

## Get started: two new pages

| Page | What changes | Source of truth | Waits for |
|---|---|---|---|
| `docs/start/index.md`, new, client tier | One page with three paths, each a short block led by its single command. **Use OML** gives the installer, `oml login`, `oml setup claude-code` and the first write. **Run OML** gives `oml quickstart`. **Add an agent** gives `oml keys issue-agent --out` and what to send the user. Each path states its target time and its "held" sentence. It replaces the README's Quickstart as the place the site sends a newcomer. | ADR-0041 d1, d6, d7, d9, d10; ADR-0039 d15 | Q (setup and quickstart), R (login) |
| `docs/start/install.md`, new, engineering tier | The installer reference covers: what each script checks (ADR-0041 d2) and both of its outcomes, word for word; the flags and variables; where it writes; the `PATH` line; the inspect-first form; ADR-0041 d5's trust notes; uninstalling (delete one file); and Windows PowerShell 5.1 and 7. The manual cosign lines move here from the CLI reference. Homebrew, Scoop and winget are listed as not offered, with ADR-0041's reasons. | ADR-0041 d1–d5; `scripts/install/` | — for the installer; Q for `--quickstart` |

## Use cases: rewritten as simplified scenarios

`docs/product/use-cases.md` (U) catalogues every call. v0.4.0 replaces the catalogue's reading
order with a few scenarios, each of five steps or fewer and one tool call per step, on the three
tools. The catalogue's detail moves to the call map, which stays engineering tier. The page is
client tier, so it uses plain words: "save", "find", "approve".

| Scenario | Steps | Waits for |
|---|---|---|
| **Remember a decision and find it next session** | `oml_write` answers `held`; `oml_read` with `include_unreviewed` returns it to its author; after a reviewer approves, every reader in the workarea finds it | R |
| **Check before acting** | `oml_read` with a query, and what to do with "nothing found" | R |
| **Correct or retire a memory** | `oml_write` with `replaces`, held like any write | R |
| **Learn whether a note was approved** | `oml_read` over the agent's own work, by state | R |
| **Two agents in one crew: one writes, one reviews** | an agent key and a reviewer key; `oml review next`, then `oml review approve` or `reject` (ADR-0039 d10) | R |
| **Give a teammate's agent its key** | `oml keys issue-agent --out`, then `oml setup claude-code --key-file` | Q |
| **Try OML on a laptop** | `oml quickstart`, then the first scenario | Q |
| **Use OML from claude.ai or a phone** | add the server as a custom connector and sign in once | O, and the open check below |

The scenarios replace the page's "Getting started" and "Everyday memory" sections. The sections
on refusals and retries move into the call map.

## Architecture

| Page | What changes | Source of truth | Waits for |
|---|---|---|---|
| [Architecture overview](../architecture/overview.md) | The context diagram gains the clients as they ship. The CLI installs per platform. The Claude Code plugin is configured through `userConfig`. The agent kit serves Codex, Cursor, Gemini CLI, VS Code and Claude Desktop. claude.ai connects through OAuth. The containers section shows the role split: the lead migrates as the owner role and serves as a role that owns nothing. A new section, "Clients and how they start", has one row per client naming its pair source. The runtime scenarios rename the MCP tools. | ADR-0034; ADR-0039 d2, d15; ADR-0040; ADR-0041; [release artifacts](ci-and-toolchain.md#release-artifacts-sbom-signing-provenance) | — for the role split and artifacts; R, P and O for their rows |
| [MCP plugin](../api/mcp-plugin.md) | The catalogue becomes three tools. "Wiring it into your agent" points to the guides below and keeps only the transport facts. | ADR-0039 d2; `docs/api/mcp-tools.json` | R |
| [Deployment](../operations/deployment.md) | The "Docker quickstart" mockup is replaced by `oml quickstart`'s real transcript, recorded from a CI run. The compose file loses `latest` and `oml_dev_only`. | ADR-0041 d9 | Q |
| [Configuration](../operations/configuration.md) | A client section naming the one pair and its precedence. | ADR-0039 d15 | R |

## CLI, plugin and kit

| Page | What changes | Source of truth | Waits for |
|---|---|---|---|
| [CLI reference](../cli/reference.md) | "Installing `oml`" leads with the installer, which it gains now, and links the install page for the manual lines. New rows: `oml login`, `oml doctor`, `oml setup claude-code`, `oml quickstart` and `oml events`. `oml context` and the renamed flags leave. Rows exist for the four built verbs that have none today (ADR-0039 context). | ADR-0039 d15, ADR-0041 d6, d7, d9; `crates/bins/oml-cli` | — for the installer; R and Q for the verbs |
| `plugins/oml/README.md` | Install through `oml setup claude-code`, or `/plugin install` and its dialog. The `export` lines leave the Claude Code path (ADR-0040 d6, checked by `check_plugin.py`). A section "Why can't I find what I just saved?" explains `held` in three sentences. | ADR-0040 d1–d6; ADR-0041 d7 | P |
| `plugins/oml-admin/README.md` | `issue-agent --out` prints the `setup claude-code --key-file` line beside the configure line. | ADR-0040 d4; ADR-0041 d7 | P, Q |
| `kit/README.md` | Each tool's file reads `OML_URL` and `OML_API_KEY`, or names its one edit. Each tool gets a three-line section, and `oml login` comes first. | ADR-0039 d15; `scripts/package_agent_artifacts.py` | R |
| [Client SDKs](sdks.md) | Generated from the served contract alone; the start settings are the pair. | ADR-0039 d15 | R |

## Guides: a new section

A guide is engineering tier and is a task from start to finish. It links the reference pages
instead of copying them.

| Guide | What it covers | Source of truth | Waits for |
|---|---|---|---|
| `docs/guides/claude-code.md` | Install, `oml login`, `oml setup claude-code`, reload, first write and read, the session-start hook and the permission rule it needs, and removal. | ADR-0029, ADR-0040, ADR-0041 d7 | R, P, Q |
| `docs/guides/claude-ai-and-mobile.md` | Adding OML as a custom connector, signing in with a connect code, and what chat and mobile get: tools, but no hooks and no skill. | ADR-0031; ADR-0040 consequences | O, and the open check below |
| `docs/guides/other-harnesses.md` | Codex, Cursor, Gemini CLI, VS Code and Claude Desktop from the agent kit; a one-line `claude mcp add --transport http` for a Claude Code user who wants no plugin; and a Cursor install link only once it can carry no key. | `kit/`; ADR-0039 d15 | R; O for the Cursor link |
| `docs/guides/self-hosting.md` | `oml quickstart` on a laptop, then the step to one host with TLS, to Kubernetes with the chart, and the docker profile's one-trust-domain caveat. | ADR-0041 d9; ADR-0023; ADR-0033; [deployment](../operations/deployment.md) | Q |
| `docs/guides/upgrading.md` | From v0.3.x to v0.4.0 for a user and for an operator: the old-to-new tool and setting table, `claude plugin update`, the one plugin configuration, the credentials file, and the server upgrade with its role split. It links [upgrades](../operations/upgrades.md) for the mechanics. | ADR-0039 d10, d12; ADR-0040 consequences; ADR-0034 d7 | R, P |

## Front pages

| Page | What changes | Source of truth | Waits for |
|---|---|---|---|
| `README.md` | The Quickstart becomes the three one-line paths, and "Connect your agent" uses `oml setup claude-code`. Python and TypeScript read the pair. | ADR-0041; ADR-0039 d15 | Q, R |
| [Home](../index.md) | "Where to go next" sends newcomers to Get started first. | — | the Get started page |
| [Concepts](../concepts.md) | No change, except tool names in examples. | ADR-0039 d2 | R |
| `docs/llms.txt` | Lines for Get started, the install page, the guides and the use cases; the rewritten tool names. | ADR-0018 d6 | each page as it lands |
| [Spec changelog](../changelog.md) | One entry per landed step, with the migration list ADR-0039 d1 requires. | ADR-0039 d1 | each step |
| `mkdocs.yml` | Two new nav sections, Get started and Guides; Get started comes first after Home. | ADR-0018 d1 | each page as it lands |

## Order of work, and the check that closes each step

1. **Now, no dependency:** the installer in the CLI reference (done), the install
   page, and the overview's role split and artifacts. Closed by the strict site build and
   `check_release.py`.
2. **With R:** the MCP plugin page, the use-case scenarios, the kit README, SDKs, configuration,
   the CLI rows for `login` and `doctor`. Closed by `check_contract_sync.py`, `check_plugin.py`
   and `check_agent_kit.py`, each rewritten by R's build to its new shape.
3. **With P:** the plugin READMEs and the Claude Code guide. Closed by `check_plugin.py` (ADR-0040
   d6).
4. **With Q's verbs:** Get started, the deployment transcript, the self-hosting guide and the README.
   Closed by `install-smoke` and by a CI run of `oml quickstart` whose output is the transcript
   the page shows and whose duration is FR-68's first measurement.
5. **With O:** the claude.ai and mobile guide, and its scenario.
6. **At the v0.4.0 tag:** the upgrading guide, the changelog's migration list, and ADR-0041 d4's
   redirect, with every page's status line checked against what the tag ships.

## Open checks before a page relies on them

- **claude.ai request headers.** A summarised fetch of the custom connector help page on
  2026-11-03 listed per-user request headers. If the primary page confirms it, claude.ai can reach
  OML with a bearer key before O lands. That would change the claude.ai guide's order and
  contradict the premise of the proposed ADR-0031. Unverified until the primary page is read and
  quoted with its fetch date.
- **One-command plugin install.** `claude plugin install <plugin> --marketplace <repo>` was
  reported in Claude Code 2.1.292 (summarised fetch, 2026-11-03). ADR-0041 d7 uses the two
  commands, which ADR-0040 d5's minimum version already covers.
- **Windows.** `install.ps1` has not run yet; `install-smoke` is its first run. The plugin's session-start hook
  is unverified without Git Bash ([CLI reference](../cli/reference.md#installing-oml-and-what-differs-on-windows)).
