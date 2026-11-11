<!--
id: adr/ADR-0041-a-quick-start-in-one-command-per-role
tier: engineering
status: accepted design · 2026-11-04
answers: how a person goes from nothing to a first memory in one command per role; what install.sh and install.ps1 download, check and write, and what they leave alone; how cosign and the checksum-only path differ and what each catches; the pinned default version and latest; where the scripts are served and what a piped install trusts; oml login and oml doctor as the onboarding step; oml setup claude-code; oml quickstart for a self-hoster; the install-smoke CI job and check_release.py's installer checks; the time-to-first-memory targets and their issue
-->

# ADR-0041 — A quick start in one command per role: an installer that checks what it downloads, `oml login`, `oml setup claude-code` and `oml quickstart`

**Status:** accepted design · 2026-11-04 · amended by [ADR-0043](ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md) · amended by [ADR-0076](ADR-0076-releases-ship-no-default-server-and-setup-asks-for-the-server-or-reads-it-from-the-key-or-the-environment.md)

**Release:** partly in v0.4.0; `install.sh --quickstart` and d4's short install address are not in it ([releases](../changelog.md))

## Context

The CLI ships per platform since v0.3.1 as `oml-cli-X.Y.Z-<target>`, signed with cosign and
summed in a signed `SHA256SUMS` ([release artifacts](https://github.com/e8kor/open-memory-lab/blob/main/docs/contributing/ci-and-toolchain.md)).
Installing it means seven lines that assume cosign ([CLI reference](../cli/reference.md#installing-oml-and-what-differs-on-windows)).
A self-hoster needs a checkout, Docker, curl and jq, and `scripts/quickstart.sh` reads the
enrollment token out of the server's log. On 2026-11-03 the maintainer asked for a minimal install
"like one for openclaw", to "get user going very quickly", in v0.4.0.

Comparable tools install in one line. OpenClaw, uv, rustup, Bun and Deno pipe a script to `sh`, and
PowerShell users pipe `install.ps1` to `iex`. Of these, only uv checks what it downloads: it pins one
SHA-256 per archive inside a script whose version is pinned. Ollama, Bun and Deno check nothing
beyond HTTPS (each installer's source, fetched 2026-11-03). The proposed
ADR-0039 d15 and ADR-0040, on branch `feat/api-rewrite`, already give every client one pair,
`OML_URL` and `OML_API_KEY`, add `oml login <url>` and `oml doctor`, and move the plugin's
settings into Claude Code's own configuration. This record decides the path a person takes
through them. Where each item stands is under [Consequences](#consequences).

## Decision

- **d1 — Two scripts install the CLI for the current user, write one file and print the next
  command.** `scripts/install/install.sh` (POSIX `sh`, Linux and macOS) and
  `scripts/install/install.ps1` (Windows PowerShell 5.1 and PowerShell 7):
    - pick the target from the OS and architecture: one of the four Unix targets, with Apple
      silicon detected under Rosetta, or `x86_64-pc-windows-msvc`, which Windows on ARM64 runs
      under emulation;
    - download the archive and `SHA256SUMS` over HTTPS, with `curl --proto '=https' --tlsv1.2`
      and TLS 1.2 forced in Windows PowerShell;
    - check them (d2), extract the binary, copy it beside the destination and rename it into
      place, then require that `oml --version` prints the requested version;
    - install into `$HOME/.local/bin` or `%LOCALAPPDATA%\Programs\oml\bin`, without sudo or
      administrator rights, with `--dir`/`-Dir` or `OML_INSTALL_DIR` to change it;
    - edit no shell profile and no `PATH`, printing the one line that would add the directory,
      and write no service;
    - print `oml login https://<your OML server>` and `oml doctor` when the installed CLI has
      `login`, and the CLI reference otherwise.

    The shell body is one function called on the last line, so a cut-short download runs
    nothing. The PowerShell script throws rather than exits, so `| iex` never closes the user's
    shell. These two scripts are the exception to the rule that a tool a person runs is an `oml`
    subcommand: they run before any `oml` exists.

- **d2 — `SHA256SUMS` always checks the archive; cosign, when present, checks `SHA256SUMS`, and
  the script says which happened.** With `cosign` on `PATH` the script fetches
  `SHA256SUMS.sigstore.json` and runs `cosign verify-blob` under the release workflow's identity,
  pinned to the exact tag (`…/release.yml@refs/tags/vX\.Y\.Z$`). It then compares the archive's
  SHA-256 with its line. Without cosign it compares the checksum only, and prints that this
  "catches a damaged download, not a tampered release". `--require-signature`/`-RequireSignature`
  or `OML_REQUIRE_SIGNATURE=1` turn a missing cosign into a refusal. Every refusal names its
  reason and installs nothing. The scripts fetch no release file except their OS's archive,
  `SHA256SUMS` and its bundle (d8).

- **d3 — The default version is the one the script was released with; `latest` is asked for.**
  The default is `DEFAULT_VERSION` in `install.sh` and `$DefaultVersion` in `install.ps1`, both
  equal to the workspace version, so the script at tag `vX.Y.Z` installs X.Y.Z, and a pasted line
  installs the same bytes tomorrow. `--version X.Y.Z` picks a release. `--version latest` reads
  `tag_name` from `GET https://api.github.com/repos/e8kor/open-memory-lab/releases/latest`, which
  skips drafts and prereleases. On `main` between a version bump and its tag the default names a
  release that does not exist yet, and the script fails naming the 404.

- **d4 — The scripts are served from GitHub at the release tag, and a short URL only redirects
  there.** The documented line for each release is
  `curl -fsSL --proto '=https' --tlsv1.2 https://raw.githubusercontent.com/e8kor/open-memory-lab/vX.Y.Z/scripts/install/install.sh | sh`,
  and on Windows `irm https://raw.githubusercontent.com/e8kor/open-memory-lab/vX.Y.Z/scripts/install/install.ps1 | iex`.
  This needs no new infrastructure. A host the project controls may serve `/install.sh` and `/install.ps1` and
  answer with a 308 redirect to the current release's URL. That is a line in the host's Caddy file,
  changed at each release, never a copy of the script. The host is the maintainer's and is set up
  at the v0.4.0 release, not in this change. The documentation never shows a `main` URL.

- **d5 — The documentation states what a piped install trusts.** The install page carries these
  notes beside the one-liner, not below the fold:
    - The pipe trusts GitHub's TLS, the repository and the tag. A tag can be moved by whoever can
      push to the repository, which is why d2's cosign check pins the tag in the identity.
    - A server can tell a pipe from a download and serve each different bytes. The page therefore
      gives the inspect-first form beside the pipe: download, read, then run with `sh install.sh`.
    - The script never asks for sudo, writes one file and edits no profile (d1).
    - d2's two outcomes are stated as they are, not as "verified".

- **d6 — After the install, `oml login <url>` is the only onboarding step, and `oml doctor`
  checks it.** Both are the proposed ADR-0039 d15's verbs and nothing is added to them. `login`
  reads the key without echo, calls `whoami` and writes `url` and `api_key` to the owner-only
  credentials file only on success. `doctor` prints each value's source, never the key, and the
  results of `/readyz` and `whoami`. The installer never runs `login` itself. The key comes from an
  administrator, and on a piped install `stdin` is the script, not the person.

- **d7 — `oml setup claude-code` installs and configures the plugin in one command, and never
  hands it an administrator's key.** The verb:
    1. takes the pair as ADR-0039 d15 resolves it (flag, environment, credentials file), with
       `--key-file <path>` for a file that `oml keys issue-agent --out` wrote;
    2. calls `whoami` and refuses, as `setup.admin_key`, a key whose role is `admin` or
       `operator`. ADR-0040 d1's `api_key` option says "never an admin key" but nothing
       enforces it, and ADR-0040 d3 only limits the harm (an admin key recalls no claim
       content); this refusal is the check, added here;
    3. requires Claude Code 2.1.285 or later (ADR-0040 d5), read from `claude --version`;
    4. runs `claude plugin marketplace add e8kor/open-memory-lab#v<its own version>` and
       `claude plugin install oml@open-memory-lab`, so the plugin matches the CLI's release.
       It first reads `claude plugin marketplace list --json`. A marketplace named
       `open-memory-lab` already at that tag is not added again, so a second run only
       reconfigures. One from the same repository at another tag, or at none, is removed and
       added at the CLI's tag, and each plugin installed from it is installed again: Claude
       Code refuses an add whose source differs from the one it holds, and the remove
       uninstalls the marketplace's plugins. One from another source, such as a local
       checkout, is refused as `setup.marketplace`, naming it, and nothing is changed;
    5. writes `{"url": …, "api_key": …}` to the stdin of `claude plugin configure
       oml@open-memory-lab --values-stdin` (ADR-0040 d4), so the key reaches neither argv nor a
       transcript;
    6. tells the user to run `/reload-plugins` or restart Claude Code.

    With `--print`, or with no `claude` on `PATH`, it runs nothing and prints the commands.
    Their configure line reads the key file, as ADR-0040 d4 shows.

- **d8 — CI runs the scripts against a published release, and `check_release.py` holds what they
  fetch.** `ci.yml` job `install-smoke` runs `install.sh` piped to `sh` on `ubuntu-24.04` and
  `macos-15`, once on Ubuntu with cosign and `--require-signature`. On `windows-2025` it runs
  `install.ps1` under PowerShell 7 and, through `Invoke-Expression`, under Windows PowerShell 5.1.
  After each it runs the installed `oml --version`. The job installs `OML_SMOKE_VERSION`, the
  newest published release, which moves at each release. `scripts/check_release.py` fails a pull
  request in any of these cases:
    - a script names a release file other than its OS's `oml-cli` archive, `SHA256SUMS` and its
      bundle;
    - it names a target the release does not build, or misses one of its OS's;
    - it never runs `cosign verify-blob` under the release identity;
    - its default version differs from the workspace version;
    - `install-smoke` loses an OS.

- **d9 — A self-hoster runs `oml quickstart`, which brings up the docker profile and prints the
  agent's key file and `login` line.** Docker with compose v2 is required. The verb:
    1. writes `compose.yaml`, embedded in the binary, with the image pinned to the CLI's own
       version and never `latest`. It also writes an owner-only `.env` with generated passwords
       for the cluster's superuser and ADR-0034's owner and serving roles, replacing
       `oml_dev_only`. Both go into `--dir`, by default `./oml`;
    2. runs `docker compose up --detach --wait`, listening on `127.0.0.1` only. The compose
       project is named `oml-` and 12 hex digits of the SHA-256 of `--dir`'s absolute path,
       in the file's `name:` and in `--project-name`. Compose otherwise names a project after
       the directory's basename, and two stacks in two `oml` directories would share their
       containers and volumes, so a `down -v` for one would delete the other's data;
    3. reads the enrollment token from the server's log, as today, until the designed
       `oml-bootstrap` one-shot service ([deployment](../operations/deployment.md#the-docker-profile))
       exists, and then from its file;
    4. claims it with `bootstrap claim --token-stdin`;
    5. creates workarea `crew`, and issues an agent key and a `dreamer` reviewer key with
       `--out` into `<dir>/keys/`;
    6. runs `scripts/quickstart.sh`'s self-check (remember, held, approve as the reviewer,
       recall);
    7. prints the agent key file, `oml login http://127.0.0.1:8080` for another machine, and
       `oml setup claude-code --url http://127.0.0.1:8080 --key-file <dir>/keys/agent` for this
       one.

    A second run against a claimed stack is refused as `quickstart.exists`, naming the directory.
    `install.sh --quickstart` then runs it after the install, so the self-hoster pastes one line.
    The flag arrives with the verb. `scripts/quickstart.sh` and its CI job move to the verb and the
    script is deleted. The docker profile is one trust domain
    ([deployment](../operations/deployment.md#the-docker-profile)); TLS and public exposure stay
    a separate guide.

- **d10 — The time to a first memory is a target, measured before it is stated as a fact.**
    - **Agent user**, with a server and a key: about two minutes from a laptop with Claude Code
      to the first write answered `held` and read back by its author (install, `login`, `setup
      claude-code`, reload).
    - **Self-hoster**, on a host with Docker: about five minutes to the quickstart's self-check,
      most of it the image pull.
    - **Administrator**, with a server running: under a minute per agent key.

    These are targets and unmeasured, tracked as #107. A memory others can recall also waits for
    a reviewer, by design, and the guides say so in the same paragraph as the target.

## Consequences

- **Where each item stands**, on `feat/v0.4.0` on 2026-11-04:

    | Item | Status |
    |---|---|
    | d1, d2 | Built. `install.sh` was run against v0.3.1 on Linux x86_64 with cosign, without it, and with `latest`. `install.ps1` runs only in `install-smoke` on Windows. |
    | d3 | Built. Both scripts default to 0.4.0, the release being cut, which is now the workspace version, so `oml setup claude-code` installs the plugin of the same release. Until the v0.4.0 tag exists, a run without `--version` fails naming the 404, as d3 says. |
    | d4 | Decided, not built: the redirect is set up at the v0.4.0 tag. |
    | d5 | Decided, not built: the install page is in the v0.4.0 documentation plan. |
    | d6 | Built with ADR-0039's `oml login` and `oml doctor`; nothing was added to them. |
    | d7 | Built: `oml setup claude-code`, with `--key-file` and `--print`, its `setup.admin_key`, `setup.claude_code_version` and `setup.marketplace` refusals, and step 4's reading of the marketplace already held. `oml-cli`'s `setup_claude_code` suite runs it against a fake `claude` that records each call and, like Claude Code, refuses an add over a marketplace declared from another source. On 2026-11-04 the `claude` commands were run by hand on Claude Code 2.1.312 against a scratch configuration directory, with tags v0.3.1 and v0.3.0, since v0.4.0 is not tagged: a repeated add at the same tag and a repeated install exit 0; an add at another tag over a held one fails, naming the declared source; `marketplace remove` uninstalls the marketplace's plugins and deletes their options, after which the add and install at the other tag succeed. The verb itself has not run against a real Claude Code, and #118's recorded session is still open. |
    | d8 | Built. |
    | d9 | Built in part: `oml quickstart` with `--dir` and `--port` and steps 1 to 7, the token read from the lead's log, and `quickstart.exists`, with `quickstart.docker` for a machine without compose. Its compose file has not run under Docker yet: CI's `compose` job runs it once the branch is pushed, and this row is restated after a green run, citing it. `oml-cli`'s `quickstart` suite runs the verb against a fake `docker` and the real lead, and asserts the compose project named in the file and in every `docker compose` call. Not built: `install.sh --quickstart`. `scripts/quickstart.sh` stays, because the `e2e` and `chart` jobs run it where there is no compose stack, so the script is not deleted yet. |
    | d10 | Unmeasured, tracked as #107. |

- **Build order for the rest:** `install.sh --quickstart` and moving the `e2e` and `chart` jobs
  off `scripts/quickstart.sh`; the `oml-bootstrap` service, from which the quickstart then reads
  the token; d5's install page with the documentation upgrade; d4's redirect at the v0.4.0 tag.
- **Pages:** the v0.4.0 documentation plan names each
  page this record changes, its source of truth and what it waits for. The [CLI
  reference](../cli/reference.md#installing-oml-and-what-differs-on-windows) gains the installer
  now and the three verbs at build.
- **Opened:** #107 (the time targets, unmeasured) and #132 (without cosign the installer cannot
  tell a tampered release from a genuine one) on the [open issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap).
- **Unverified, and read before any guide relies on it:** a summarised fetch of claude.ai's
  custom connector help page, on 2026-11-03, listed per-user request headers. That would let
  claude.ai reach OML with a bearer key and no OAuth, which contradicts the premise of the
  proposed ADR-0031. The primary page decides.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Hashes written into the script, as uv does** | The script at a tag cannot hold the hashes of archives that tag's workflow builds later. It would have to be generated after the build and attached to the release, and the signed `SHA256SUMS` already carries the same hashes under cosign. |
| **Homebrew tap, Scoop bucket or winget first** | Since Homebrew 6.0.0 a third-party tap needs `brew trust` before it installs ([Tap trust](https://docs.brew.sh/Tap-Trust), fetched 2026-11-03), and winget publishes through a reviewed pull request per release. Each can follow the script. None is one command on every OS today. |
| **An npm or `npx` wrapper** | It makes Node a dependency of a static binary. |
| **Edit the shell profile by default, as uv, Bun and OpenClaw do** | A tool that guards memory writes should write one file. The installer prints the line instead, and a user who wants the edit makes it. |
| **Run onboarding at the end when a TTY exists, as OpenClaw does** | The key comes from an administrator, not from the install. On a piped install `stdin` is the script, and an interactive step would also make the script harder to test in CI. |
| **Install to `/usr/local/bin` with sudo, with a service, as Ollama does** | The CLI is a client. The server's quick path is compose (d9), and production on one host is its own guide. |
| **`latest` by default** | The same pasted line would install different bytes on different days, and an error report could not name its version. |
| **Require cosign** | Most users have no cosign, and refusing them sends them to a manual path that checks nothing. The checksum-only path says what it does not catch, and #132 tracks the gap. |
| **Serve a copy of the script from a project host** | A second copy has to be kept in step with the tag. A redirect keeps the bytes at the tag. |
| **Keep the self-host quick start a shell script** | `scripts/quickstart.sh` needs a checkout and jq and reads the token by matching log text. The maintainer's rule is that a tool a person runs is an `oml` verb. |
