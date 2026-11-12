<!--
id: engineering/ci-and-toolchain
tier: engineering
status: design draft for review · decided, not built
answers: which toolchain builds OML, what gates every PR, how the supply-chain claim is enforced, what a release ships and how it is verified
-->

# CI and toolchain

**Status:** design draft for review · decided, not built

This page decides the toolchain pins, the merge gates and the release machinery before any
pipeline exists. The reason it exists now is [ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md):
rejecting dynamic plugin loading moved the entire supply chain "inside `cargo build`", and a
supply chain inside the build is only a security property if the build is pinned, audited and
attested — which is this page's job to specify. The first pipeline is an M1 item on the
critical path. Since 2026-10-30 `.github/workflows/ci.yml` runs
the format, lint, test, docs, cargo-deny and end-to-end gates, and `release.yml` ships the signed
artifacts below; since 2026-10-31 `ci.yml` also runs the three jobs of the
[client SDKs](../engineering/sdks.md), and `m0-spikes.yml` holds the M0 spike gate below. Not running yet: the
MSRV build, the sqlx offline check, container tests against MinIO, the scheduled `cargo audit` and
the bench job.

## Toolchain pins and MSRV

- **`rust-toolchain.toml` pins one exact stable release** at the workspace root; every build —
  developer, CI, release — uses that pin and nothing else. The pinned version is a repository
  fact, not a documentation fact: this page states the policy, the file states the number, so the
  number has one declaration site (the [ADR-0018](../adr/ADR-0018-the-documentation-set.md) d5
  rule applied to a version string).
- **MSRV policy:** the minimum supported Rust version is the pinned stable **minus a small, stated
  window — two minor releases** — recorded as `rust-version` in the workspace manifest and
  verified by a CI job that builds the workspace on the MSRV toolchain. The window exists so
  downstream packagers are not forced onto release-day toolchains; it is kept small because an
  old-toolchain support promise is a cost, and nobody has asked OML to pay a large one.
- **MSRV bumps are changelog entries**, never silent: a bump names the release that raised it and
  the feature that needed it.
- `Cargo.lock` is **committed**. The workspace ships binaries
  ([ADR-0006](../adr/ADR-0006-pure-rust.md) d6), and for binaries the lockfile is the build: an
  unlocked dependency graph would make the SBOM below a fiction.

## Merge gates

Every PR passes all of these, in the workspace, before merge — "warnings allowed for now" is a
ratchet that only ever loosens, so it starts tight:

| Gate | Command shape | What it holds |
|---|---|---|
| Format | `cargo fmt --check` | one style, no diff noise |
| Lints | `cargo clippy --all-targets --all-features` with warnings denied | the workspace lint table (workspace plan) is enforced, not advisory |
| Tests | `cargo test --workspace` | domain properties and port contracts against doubles ([test strategy](test-strategy.md)); fast, container-free |
| Container tests | the integration and adapter-contract jobs | real Postgres + MinIO at the [deployment](../operations/deployment.md) pins |
| Docs | `cargo doc` with `RUSTDOCFLAGS="-D warnings"` | every public item documented; broken intra-doc links fail |
| Site | `mkdocs build --strict` | this documentation set keeps building; a broken link is a failure, not a warning |
| Chart | `scripts/check_chart.py`, then job `chart`: a kind cluster, `pgvector/pgvector:pg17` in it, the image built from the checkout, `helm install`, the claim through `kubectl port-forward`, `scripts/quickstart.sh`, a restart, and an uninstall and reinstall | the Helm chart renders the one lead [ADR-0033](../adr/ADR-0033-interim-helm-chart.md) decides, at the workspace version and as the image's own user, and that lead serves the quickstart after a real install and keeps its signing key across a restart and a reinstall |
| Quick start | job `compose`: the repository's `docker-compose.yml` built and started, `scripts/quickstart.sh` inside it, then `oml quickstart` from a release build against the same image tagged with the CLI's version, the owner-only modes of what it wrote, and a second run refused as `quickstart.exists` | the compose file `oml quickstart` writes starts the lead over its owner and serving roles, and the verb claims it, issues its keys and passes its self-check ([ADR-0041](../adr/ADR-0041-a-quick-start-in-one-command-per-role.md) d9) |
| CLI platforms | job `cli-matrix`: `cargo build --bin oml`, `oml --version` and `cargo test -p oml-cli --lib --bins`, each on the native runner of one CLI release target | every platform the CLI ships for still builds and passes its unit tests, before a tag ([release artifacts](#release-artifacts-sbom-signing-provenance)) |
| Installers | job `install-smoke`: `scripts/install/install.sh` piped to `sh` on `ubuntu-24.04` (once with cosign and `--require-signature`) (macOS paused), `install.ps1` under PowerShell 7 and Windows PowerShell 5.1 on `windows-2025`, each against the newest published release, then the installed `oml --version`; `shellcheck` on `install.sh` | the one-line install still works on each OS, against real signed release files ([ADR-0041](../adr/ADR-0041-a-quick-start-in-one-command-per-role.md) d8) |
| Schema sync | sqlx offline check: compile-time-checked queries against committed query metadata, regenerated whenever `migrations/` changes | queries and migrations cannot drift apart silently; the companion shape-vs-bytes rule is declared in the [schema page](https://github.com/e8kor/open-memory-lab/tree/main/migrations) |
| M0 spikes | `uv run --locked --script scripts/check_chain_vectors.py`; `uv run --locked --script testing/spikes/rls/spike_rls.py --require-all` against `pgvector/pgvector:pg17` | the chain-format vectors keep reproducing outside Rust and the Rust tests' copies of them keep equalling the page, and spike 3's findings keep standing, recorded failures included ([M0 spikes](https://github.com/e8kor/open-memory-lab/blob/main/design/research/spikes-index.md)) |

## Which jobs a change runs

On a pull request into `main`, `scripts/ci_paths.py` sorts the changed paths into categories, and each
workflow's `changes` job (`changed paths`) passes them to the jobs, which skip when their category is
unchanged. A push to `main`, a merge group, a pull request into another branch, a change under
`.github/`, `scripts/`, `Cargo.lock` or `rust-toolchain.toml`, and a path that matches no category
each run everything. `scripts/tests/test_ci_paths.py` holds the table below and checks the workflows
against it.

| Category | Paths | Jobs it turns on |
|---|---|---|
| `rust` | `crates/`, `sdks/rust/` (the workspace member `oml-sdk`), `Cargo.toml`, `deny.toml`, `testing/lake/`, `testing/oml-port-contracts/` | `cargo` |
| `migrations` | `migrations/` | `cargo`, `spike 3, RLS hardening` |
| `api` | `docs/api/*.yaml`, `docs/api/*.json` (the server compiles in and its tests read them) | `cargo`, `openapi.yaml validates`, `generated SDK code is current` |
| `cargo` | `rust`, `migrations` or `api` | fmt, clippy and doc; `cargo test` with Postgres; `crate walls (oml-web)`; `cargo deny`; `demo and quickstart`; `oml CLI` on three targets; the SDK jobs; the recall eval; the release image, `compose` and `chart` |
| `docs` | `docs/`, `design/`, `mkdocs.yml`, `requirements.txt`, `changes/`, root `*.md`, licences, `hooks/` (the mkdocs build hook), `site/` (the landing site) | `mkdocs build --strict`, benchmark pages, `contract surfaces agree`, generated references, chain-format vectors, and `docs_tests` |
| `docs_tests` | `docs` or `eval` | the Rust targets that read those files, unless `cargo` already runs the workspace |
| `cli_ref` | `docs/cli/` | `CLI reference matches the oml binary's help` |
| `eval` | `eval/`, `bench/` | the recall eval and its data check, remember eval data, benchmark pages, and `docs_tests` |
| `plugins` | `plugins/`, `kit/`, `.claude-plugin/` | `agent plugin agrees with the server`, `contract surfaces agree` |
| `sdk` | `sdks/` | the Python, TypeScript and generated-code SDK jobs, `SDK packages build, ship the right files and install`, `contract surfaces agree` |
| `release` | `RELEASE-HOLD`, `Dockerfile`, `.dockerignore`, `docker-compose.yml`, `deploy/`, `LICENSES/` (the image copies it) | the release image, `compose`, `chart` and `install-smoke` |
| `spikes` | `testing/spikes/` | `spike 3, RLS hardening` |

The Rust tests that read files outside `crates/` are the reason `docs` and `eval` keep a slice of
the Rust suite: `oml-scan`'s `scan` (every page under `docs/`, and `eval/recall/corpus.json`), the
roles-and-keys footnote test in `oml-transport-http`, `oauth_endpoints` against
`docs/reference/connect-page.md`, `cli_e2e` against `docs/cli/reference.md`, `sync_extraction`
against `docs/adr/`, `near_duplicates` and the lake writer's bench-results load against `eval/`.
Job `cargo test, the targets that read docs and eval` runs exactly those; a new test that reads a
path outside `crates/` adds its target there and its path to `CATEGORIES`.

The practices workflow has no filter: its jobs are cheap and the lints must always run. Each of
`ci.yml`, `docs-checks.yml`, `m0-spikes.yml` and `practices.yml` ends in a job named `ci ok (ci)`, `ci ok (docs)`, `ci ok (spikes)` and `ci ok (practices)` respectively, which
needs every job of its workflow and fails only when one failed or was cancelled, so a skipped job
never blocks it. Requiring those four checks, by those four distinct names, instead of each job is a repository setting; one name shared by four workflows would let a green check from one satisfy the rule for another.

### The required checks

Branch protection on `main` requires five checks, by these names:

| Check | Workflow | What a red one means |
|---|---|---|
| `ci ok (ci)` | `ci.yml` | a code, test, build, image, chart or SDK job failed |
| `ci ok (docs)` | `docs-checks.yml` | the site, a generated page or a contract surface disagrees |
| `ci ok (spikes)` | `m0-spikes.yml` | the chain vectors or spike 3 no longer reproduce |
| `ci ok (practices)` | `practices.yml` | a repository lint failed |
| `crate walls (oml-web)` | `ci.yml` | `oml-web` reaches an application crate, or the lead serves a page route ([ADR-0070](../adr/ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md) d7 (d)) |

`ci ok (ci)` already waits for `crate walls (oml-web)`; requiring the walls by their own name as well
keeps them named on every pull request and keeps a change to `ci-ok` from dropping them silently.
A job skipped by its path filter reports success, so each required check reports on every pull
request. Each of the four workflows also runs on `merge_group`, so the same checks gate a merge
queue (`scripts/tests/test_ci_paths.py` fails a workflow with a final check that does not).

A new job joins its workflow's `ci-ok` `needs` list, which `test_ci_paths.py` checks, and is then
required through it; it needs a required check of its own only when a decision names it, as
ADR-0070 d7 (d) names the walls. A required check's name is a repository setting, so renaming one
is a change to branch protection in the same pull request, and `test_every_check_name_survives`
lists every name that must not change by accident.

## Supply chain: cargo-deny and cargo-audit

These two jobs are the enforcement of ADR-0019's claim. Compiled-in plugins mean the attack
surface is exactly the crate graph, so the crate graph is what gets audited:

- **`cargo deny check`** runs on every PR with a committed `deny.toml` covering all four checks:
  **advisories** (RUSTSEC database), **licenses** (allowlist below), **bans** (duplicate-version
  and denied-crate rules — one adapter crate per infrastructure dependency is
  [ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md) d2's rule, and the bans list
  is where a second Postgres driver gets refused), and **sources** (crates.io only; no git
  dependencies in release builds).
- **`cargo audit`** runs on a schedule against the committed `Cargo.lock`, so an advisory
  published between PRs still surfaces within a day, not at the next merge.
- **License policy** (a bound decision, stated in the README's License section): the repository
  and all code are **Apache-2.0**; the `docs/` content is additionally **CC-BY-4.0**. The
  `deny.toml` allowlist admits Apache-2.0-compatible dependency licenses and nothing copyleft
  that would encumber the binaries.

An advisory with no fixed release is triaged, not auto-ignored: an ignore entry in `deny.toml`
requires a comment naming the advisory, the reason, and the revisit condition — the same
named-trigger discipline the ADRs use.

## Pinned actions

Every `uses:` in `.github/` names a commit, not a tag, and the tag it was resolved from follows in
a comment: `uses: actions/checkout@<40 hex> # v4.4.0`. A tag is a pointer its owner can move to
other code after the pin is reviewed; a commit cannot move. One action is one commit in every
workflow, so a version bump is one change that every job takes together.
`python scripts/lint_practices.py actions`, in the practices workflow's lints job, fails a pull
request that adds a `uses:` without a 40-hex SHA, a pin without its `# v<tag>` comment, or a second
commit for an action already pinned. The lint parses each workflow as YAML and reads every `uses`
key from the parsed file, so a key in a flow mapping, a quoted key, a value on the next line or an
anchor is read as any other; a file that does not parse fails. One action is one repository: a
sub-path such as `actions/cache/restore` is pinned to the commit `actions/cache` is.

To add or change a pin, resolve the tag to a commit and write both:

```sh
gh api repos/<owner>/<repo>/git/ref/tags/<tag> --jq .object        # type "commit": that sha is the pin
gh api repos/<owner>/<repo>/git/tags/<sha> --jq .object.sha        # type "tag" (annotated): dereference once
```

The weekly dependency pass runs `scripts/check_action_updates.py`, which lists every pinned action
with a newer `vN.M.P` tag. A newer tag of the pinned major is a finding in the weekly issue: take it
by changing every use of that action in one pull request, with the same resolve step. A newer major
is printed in the job log and is not a finding, because a major can change inputs, the runner's Node
version or defaults, and gets its own reviewed pull request. No bot opens these pull requests; the
weekly issue is the reminder.

## Release artifacts: SBOM, signing, provenance

A release is not a tag; it is a set of verifiable artifacts. Per release:

- **SBOM:** one CycloneDX SBOM per binary and per container image, generated from the committed
  lockfile (cargo-cyclonedx), published next to the artifact it describes.
- **Signing:** every released binary, container image and SBOM is signed with **sigstore cosign**
  (keyless, bound to the repository's release workflow identity). The verification command is
  part of the release notes, and its stated form is:

    ```
    cosign verify \
      --certificate-oidc-issuer https://token.actions.githubusercontent.com \
      --certificate-identity-regexp '^https://github.com/e8kor/open-memory-lab/\.github/workflows/release\.yml@refs/tags/.*$' \
      <image-or-blob-reference>
    ```

    A release whose artifacts do not verify with that command is not a release. The identity
    regexp is normative in shape; the exact workflow path is fixed when the workflow exists.
- **One image, two architectures (built, not yet released).** The image
  `ghcr.io/e8kor/open-memory-lab:<X.Y.Z>` is one OCI image index carrying `linux/amd64` and
  `linux/arm64`. `.github/workflows/release.yml` builds each platform on a native runner (job
  `image`) — `ubuntu-24.04` and `ubuntu-24.04-arm`, a label GitHub documents for public and
  private repositories
  ([GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners),
  fetched 2026-10-30) — and pushes it by digest with its own SBOM and provenance. QEMU emulation
  is the fallback should the label not serve this repository: the arm64 leg's runner becomes
  `ubuntu-24.04`, and the job's `docker/setup-qemu-action` step, which runs only on a runner
  not native to its platform, emulates `linux/arm64`. A merge job (`merge`) assembles the index
  with `docker buildx imagetools create`, asserts that both platforms are in it, and only then
  signs the **index digest** with `cosign sign --recursive`, which also signs each platform
  image in it, so the verification command above covers both architectures. In CI, the `image` job of
  `.github/workflows/ci.yml` builds both platforms on every pull request and runs the image's
  `oml --version` on each, and `scripts/check_release.py` fails the pull request if
  `release.yml` stops building both platforms, assembling the index or signing its digest. No
  tag has run this pipeline yet — the `v0.1.0` image is `linux/amd64` alone, as that tag's
  `release.yml` builds it — so no page claims an arm64 release image until a tag's
  `docker buildx imagetools inspect` lists both platforms and `cosign verify` passes on its
  index digest. A tag's release notes name both platforms because that run's merge job asserted
  them before signing.
- **The CLI and server archives (built, not yet released).** The CLI ships apart from the server,
  a decision the maintainer made on 2026-10-31. A tag ships `oml-cli-X.Y.Z-<target>.tar.gz`, the
  `oml` binary with the README and licence, for `x86_64-unknown-linux-musl` and
  `aarch64-unknown-linux-musl` (static, so they run on any distribution) and
  `oml-cli-X.Y.Z-x86_64-pc-windows-msvc.zip` (the macOS targets are [paused](#paused-targets)); and
  `oml-server-lead-X.Y.Z-<target>.tar.gz` for `x86_64-unknown-linux-gnu` and
  `aarch64-unknown-linux-gnu`; and, from v0.6.0, `oml-lakewriter-X.Y.Z-<target>.tar.gz` for the same
  two gnu targets, the lake writer's own service (ADR-0060); and `oml-web-X.Y.Z-<target>.tar.gz`
  for the same two gnu targets, the web console's own process (ADR-0070 d4), which job `web`
  packages only after the binary prints the tag's version. `release.yml` job `cli` builds each CLI target on a runner of its
  own OS and architecture — `ubuntu-24.04`, `ubuntu-24.04-arm` and
  `windows-2025`, labels GitHub lists for public repositories
  ([GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners),
  fetched 2026-11-03) — and packages it only after the binary prints the tag's version; a musl
  binary must also carry no program interpreter. Both musl legs compile ring's C with `musl-gcc`, the
  wrapper Ubuntu's `musl-tools` ships for the runner's own architecture, named in
  `CC_x86_64_unknown_linux_musl` and `CC_aarch64_unknown_linux_musl` of the job's env, and link
  Rust's self-contained musl with the runner's own `cc`, so no cross toolchain is pinned.
  `scripts/musl_toolchain.sh` installs `musl-tools` under a time limit and, when Ubuntu's mirrors
  cannot be reached (every run on 2026-11-11 failed resolving them), puts a `musl-gcc` on the
  path that runs Zig 0.15.2's clang and musl from the `ziglang` wheel on PyPI, checked against its
  SHA-256, so the jobs' `CC_<target>` and `cargo build` lines are the same on either path. The
  name is needed on the arm64 runner: the `cc` crate (1.10.0 in `Cargo.lock`) counts
  `aarch64-unknown-linux-musl` as a cross compile there and would otherwise call
  `aarch64-linux-musl-gcc`, which `musl-tools` does not ship
  ([noble arm64 file list](https://packages.ubuntu.com/noble/arm64/musl-tools/filelist),
  fetched 2026-11-03); `scripts/check_release.py` requires both variables in job `cli` and in
  `ci.yml` job `cli-matrix`. Job `server` builds the server on `ubuntu-22.04` and
  `ubuntu-22.04-arm`, whose glibc is the oldest the listed Linux runners carry. Job `binaries`, once
  every archive exists, writes one SBOM per archive with `cargo cyclonedx --target <target>`
  (`<archive stem>.cdx.json`), one `SHA256SUMS` over every archive and SBOM, and then signs every
  archive, every SBOM and `SHA256SUMS` with `cosign sign-blob`, under the identity above. In CI,
  job `cli-matrix` of `.github/workflows/ci.yml` builds `oml` on the same target and runner pairs
  on every pull request, runs its `--version` and runs the CLI's unit tests, the Windows
  owner-only file tests among them ([CLI reference](../cli/reference.md#installing-oml-and-what-differs-on-windows));
  the end-to-end suite stays on Linux, where Postgres runs. `scripts/check_release.py` fails the
  pull request if `release.yml` stops building, packaging, summing or signing any target, or if
  `cli-matrix` stops covering one. No tag has run these jobs yet, and until a pull request runs
  `cli-matrix`, no macOS or Windows build of `oml` has run anywhere.
- **The Helm chart (built, not yet released).** A tag also ships `oml-X.Y.Z.tgz`, the chart at
  `deploy/helm/oml` ([ADR-0033](../adr/ADR-0033-interim-helm-chart.md) d10), and its
  `.sigstore.json` bundle. `release.yml` job `chart` checks that the chart's `version` and
  `appVersion` equal the tag, packages it and signs the archive with `cosign sign-blob`; job
  `chart-release`, once the release exists, uploads both files and adds the install and
  `cosign verify-blob` lines to the release notes. `scripts/check_release.py` fails the pull
  request if `release.yml` stops doing any of that. No chart repository is published, and no tag
  has run these jobs yet.
- **The agent artifacts (built, not yet released).** A tag also ships five files, which the
  maintainer asked for on 2026-11-03, all written by `scripts/package_agent_artifacts.py` from the
  repository's tracked files:
    - `oml-claude-plugins-X.Y.Z.tar.gz` and `.zip`: `.claude-plugin/marketplace.json`,
      `plugins/oml`, `plugins/oml-admin` and the licence in one directory. The marketplace's
      sources are relative paths, which resolve from the marketplace root, the directory holding
      `.claude-plugin/`, so `/plugin marketplace add <extracted directory>` installs both plugins
      with no network; Claude Code reads a relative-path plugin from that directory in place, so
      the directory stays where it was extracted
      ([Create a marketplace](https://code.claude.com/docs/en/plugin-marketplaces) and the
      [marketplace reference](https://code.claude.com/docs/en/plugins/marketplace-reference),
      both fetched 2026-11-03).
    - `oml-marketplace-X.Y.Z.json`: the same marketplace with each source a `git-subdir` of this
      repository, its `ref` the tag and its `sha` the tag's commit; when both are set, Claude Code
      checks out the `sha` (marketplace reference, fetched 2026-11-03). A marketplace added by URL
      is downloaded as that one file, so no source in it may be relative; the release notes add
      this one from a directory instead, a `directory` marketplace source. A GitHub marketplace is
      pinned with `#<ref>` too, as in `claude plugin marketplace add e8kor/open-memory-lab#vX.Y.Z`
      ([Host and maintain a marketplace](https://code.claude.com/docs/en/plugins/host-marketplace),
      fetched 2026-11-03). Every form names the marketplace `open-memory-lab`, and Claude Code
      registers one marketplace per name, so a user holds one of them at a time.
    - `oml-agent-kit-X.Y.Z.tar.gz` and `.zip`: MCP configurations for OpenAI Codex CLI, Cursor,
      Gemini CLI, VS Code and Claude Desktop, each naming `/mcp` with the key from `OML_API_KEY`,
      Cursor's and Gemini CLI's at `${OML_URL}/mcp` since those two document expanding a variable
      in a URL, the other three at the literal `http://127.0.0.1:8080/mcp` (ADR-0039 d15); an
      `AGENTS.md` block whose sections are the oml plugin's skill, verbatim; a Gemini CLI
      `SessionStart` hook running the plugin's guarded `oml hook session-start` command; and
      `kit/README.md`, which cites each tool's documentation with its fetch date and marks what
      that documentation does not settle as unverified. Cursor's `sessionStart` hook reads and
      writes other fields than the verb does, so the kit wires none there.

    `release.yml` job `agents` runs `scripts/check_agent_kit.py`, then packages the five files
    with the tag's version, the repository and the tag's commit. Job `binaries` needs it, requires
    each of the five among the files it downloads, lists them in `SHA256SUMS` and signs each with
    `cosign sign-blob` under the identity above. The archives are deterministic: sorted entries,
    fixed timestamps and modes. In CI, job `plugin` of `ci.yml` runs `scripts/check_agent_kit.py`,
    which builds the five files, reads both archives back and fails if a `.tar.gz` and its `.zip`
    differ, if a plugin source does not resolve inside the archive, if the pinned marketplace names
    another ref, if a kit configuration does not reach `/mcp` with `OML_API_KEY` in its tool's
    environment form or holds a literal key, if the `AGENTS.md` block differs from the skill, or if
    the Gemini CLI hook is not the plugin's command. `scripts/check_release.py` fails the pull
    request if `release.yml` stops packaging, requiring, summing or signing them. No tag has run
    job `agents` yet.
- **The landing site archive (built, not yet released).** A tag also ships
  `oml-site-X.Y.Z.tar.gz`: the landing pages of `site/dist/` and the licences under
  `oml-site-X.Y.Z/`, static files for a web server to serve
  ([site/README.md](https://github.com/e8kor/open-memory-lab/blob/main/site/README.md)).
  `release.yml` job `site` runs `scripts/gen_design_tokens.py --check` and `site/build.py --check`,
  so the archive holds what the tag's sources build, and packs it with sorted entries and a fixed
  owner. Job `binaries` needs it, requires it among the files it downloads, lists it in
  `SHA256SUMS` and signs it with `cosign sign-blob` under the identity above.
  `scripts/check_release.py` fails the pull request if `release.yml` stops packaging, requiring,
  summing or signing it. No tag has run job `site` yet.
- **The SDK packages (built, not yet released).** A tag also ships the Python SDK's wheel and sdist and
  the TypeScript SDK's npm tarball, at the server's version, named by `scripts/sdk_names.py` from
  their manifests. `release.yml` job `version` runs `scripts/check_sdk_versions.py` with the tag, so a
  tag whose SDK manifests were not bumped with the server's stops before anything is built. Job `sdks`
  runs `scripts/build_sdk_packages.py`: it builds each package, reads the metadata and the file list
  back out of the built file (name, version, licence, readme, repository URL; no test, fixture, lock
  file or key), installs it into a fresh environment away from the checkout and imports it. Job
  `binaries` needs it, lists the three files in `SHA256SUMS` and signs each with `cosign sign-blob`.
  Two further jobs, `publish-sdk-pypi` (PyPI trusted publishing, no stored token) and
  `publish-sdk-npm` (npm, with provenance), upload those same signed files after the GitHub release
  exists. They are skipped unless the repository variable `PUBLISH_SDKS` is `true`, stop at their
  first step while `sdks/REGISTRY-NAMES.toml` does not record a decided registry name, and check each
  file by name against `SHA256SUMS` first
  ([releasing](releasing.md#publishing-the-sdks)). CI's `sdk-packages` job runs the build script on
  every pull request that touches `sdks/`. `scripts/check_release.py` fails the pull request if the
  files stop being built, summed or signed, if a publish job loses its gate, its name guard, its
  per-file checksum check, its environment or its commit-pinned actions, gains `continue-on-error`,
  `|| true` or a step-level `if:`, uploads before it checks, gives the PyPI action any input but
  `print-hash`, writes its name guard or its checksum line any way but whole, fills `dist/` in the
  PyPI job by anything but the copy of the two checked files, checks out anything but the tag, or publishes an npm path without
  `./`, or if any other job uploads to a registry (`npm`, `pnpm` or `yarn` publish in any spelling,
  `twine upload`, `uv publish`). No
  tag has run these jobs yet.
- **SLSA provenance is a target, not a claim.** The intended level is SLSA Build L2-equivalent —
  provenance generated by the hosted build, signed, covering source revision and build
  parameters. Until a release actually ships with verifiable provenance, no page calls OML
  SLSA-anything; this sentence is the marker.
- Release versioning starts where the spec is: the documentation set is versioned `0.7.1-design`
  (matching the [OpenAPI document](../api/openapi.yaml)'s `info.version`); code releases begin
  their own series when code exists, and the provider ring's compatibility covenant
  (#150) binds versioning from the v1-stable declaration, not before.

## Bench-regression job

CI carries a bench job wired to the [benchmark harness](benchmark-harness.md):

- On merge to main, the **criterion micro-benchmarks** (tier 1) run and emit the harness's
  machine-readable report; the job diffs against the stored baseline **for the same environment
  id** and flags regressions beyond the harness's stated threshold.
- The job is **advisory on shared runners** — hosted CI hardware is unpinned and noisy, and a
  number from an unpinned environment is not comparable to anything, per the harness's
  environment-pinning rule. Authoritative regression verdicts come only from harness runs on the
  pinned rig environment; the CI job exists to catch order-of-magnitude mistakes early, and its
  advisory status is stated here so nobody upgrades it into a fact.
- No benchmark number from this job ever appears in documentation; every published number goes
  through #150's publication rule (date, hardware, settings).

## What CI deliberately does not do

- No nightly-toolchain features anywhere: the pinned stable is the only toolchain, which keeps
  the MSRV promise honest and the build reproducible.
- No auto-publish of measurements: harness reports are artifacts a person publishes under the
  the documentation rules, never pipeline side effects.
- No doc deployment on PR branches: the site deploys from the default branch only, so an
  unreviewed page is never public.

## Paused targets

**macOS, since 2026-11-04.** `aarch64-apple-darwin` and `x86_64-apple-darwin` are out of `cli-matrix`,
`install-smoke` and `release.yml`'s `cli` job, by the maintainer's decision: GitHub's macOS runners
were repeatedly cancelled without a runner during the v0.5.1 and v0.5.2 releases, and a release waits
for every target. Releases up to v0.5.2 carry macOS archives; later ones do not. `install.sh` stops on
macOS with a message naming `cargo install` from source, and `check_release.py` no longer requires a
macOS runner. To restore: put the two rows back in both matrices and `CLI_TARGETS`, the `macos-15`
runner back in `install-smoke`, and the `Darwin` branch of `install.sh`.
