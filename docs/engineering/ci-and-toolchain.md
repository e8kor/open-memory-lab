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
[critical path](../product/critical-path.md). Since 2026-10-30 `.github/workflows/ci.yml` runs
the format, lint, test, docs, cargo-deny and end-to-end gates, and `release.yml` ships the signed
artifacts below; since 2026-10-31 `ci.yml` also runs the three jobs of the
[client SDKs](sdks.md), and `m0-spikes.yml` holds the M0 spike gate below. Not running yet: the
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
| Lints | `cargo clippy --all-targets --all-features` with warnings denied | the workspace lint table ([workspace plan](workspace-plan.md)) is enforced, not advisory |
| Tests | `cargo test --workspace` | domain properties and port contracts against doubles ([test strategy](test-strategy.md)); fast, container-free |
| Container tests | the integration and adapter-contract jobs | real Postgres + MinIO at the [deployment](../operations/deployment.md) pins |
| Docs | `cargo doc` with `RUSTDOCFLAGS="-D warnings"` | every public item documented; broken intra-doc links fail |
| Site | `mkdocs build --strict` | this documentation set keeps building; a broken link is a failure, not a warning |
| Chart | `scripts/check_chart.py`, then job `chart`: a kind cluster, `pgvector/pgvector:pg17` in it, the image built from the checkout, `helm install`, the claim through `kubectl port-forward`, `scripts/quickstart.sh`, a restart, and an uninstall and reinstall | the Helm chart renders the one lead [ADR-0033](../adr/ADR-0033-interim-helm-chart.md) decides, at the workspace version and as the image's own user, and that lead serves the quickstart after a real install and keeps its signing key across a restart and a reinstall |
| Schema sync | sqlx offline check: compile-time-checked queries against committed query metadata, regenerated whenever `migrations/` changes | queries and migrations cannot drift apart silently; the companion shape-vs-bytes rule is declared in the [schema page](../data/postgres-schema.md#migrations) |
| M0 spikes | `uv run --locked --script scripts/check_chain_vectors.py`; `uv run --locked --script testing/spikes/rls/spike_rls.py --require-all` against `pgvector/pgvector:pg17` | the chain-format vectors keep reproducing outside Rust and the Rust tests' copies of them keep equalling the page, and spike 3's findings keep standing, recorded failures included ([M0 spikes](spikes/index.md)) |

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
  image in it, so the verification command above covers both architectures. The binary
  tarballs stay `x86_64-linux` only, and the release notes say so. In CI, the `image` job of
  `.github/workflows/ci.yml` builds both platforms on every pull request and runs the image's
  `oml --version` on each, and `scripts/check_release.py` fails the pull request if
  `release.yml` stops building both platforms, assembling the index or signing its digest. No
  tag has run this pipeline yet — the `v0.1.0` image is `linux/amd64` alone, as that tag's
  `release.yml` builds it — so no page claims an arm64 release image until a tag's
  `docker buildx imagetools inspect` lists both platforms and `cosign verify` passes on its
  index digest. A tag's release notes name both platforms because that run's merge job asserted
  them before signing.
- **The Helm chart (built, not yet released).** A tag also ships `oml-X.Y.Z.tgz`, the chart at
  `deploy/helm/oml` ([ADR-0033](../adr/ADR-0033-interim-helm-chart.md) d10), and its
  `.sigstore.json` bundle. `release.yml` job `chart` checks that the chart's `version` and
  `appVersion` equal the tag, packages it and signs the archive with `cosign sign-blob`; job
  `chart-release`, once the release exists, uploads both files and adds the install and
  `cosign verify-blob` lines to the release notes. `scripts/check_release.py` fails the pull
  request if `release.yml` stops doing any of that. No chart repository is published, and no tag
  has run these jobs yet.
- **SLSA provenance is a target, not a claim.** The intended level is SLSA Build L2-equivalent —
  provenance generated by the hosted build, signed, covering source revision and build
  parameters. Until a release actually ships with verifiable provenance, no page calls OML
  SLSA-anything; this sentence is the marker.
- Release versioning starts where the spec is: the documentation set is versioned `0.2.0-design`
  (matching the [OpenAPI document](../api/openapi.yaml)'s `info.version`); code releases begin
  their own series when code exists, and the provider ring's compatibility covenant
  ([FR-21](../product/roadmap.md)) binds versioning from the v1-stable declaration, not before.

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
  through [FR-25](../product/roadmap.md)'s publication rule (date, hardware, settings).

## What CI deliberately does not do

- No nightly-toolchain features anywhere: the pinned stable is the only toolchain, which keeps
  the MSRV promise honest and the build reproducible.
- No auto-publish of measurements: harness reports are artifacts a person publishes under the
  roadmap's rules, never pipeline side effects.
- No doc deployment on PR branches: the site deploys from the default branch only, so an
  unreviewed page is never public.
