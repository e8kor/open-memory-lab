"""The release workflow ships one signed image index, the signed chart, signed CLI, server and oml-web archives, and the signed agent artifacts.

Run by .github/workflows/ci.yml (job image). Holds .github/workflows/release.yml
to docs/engineering/ci-and-toolchain.md "Release artifacts", failing with every
disagreement found:

- the version job runs scripts/check_release_hold.py as its first step, and
  every other job needs the version job, so RELEASE-HOLD at the root stops
  the whole release;
- the image is built for linux/amd64 and linux/arm64, each build pushed by
  digest rather than by tag;
- a merge job, needing the build job, assembles the index with
  `docker buildx imagetools create`;
- that merge job signs the index digest with `cosign sign --recursive`, after it
  has asserted both platforms with `docker buildx imagetools inspect --raw`, and
  no build job signs a single-platform digest;
- every image build passes OML_BUILD_COMMIT and OML_BUILD_COMMIT_TIME, since the
  image context has no .git and /readyz names the build it serves
  (docs/api/http-api.md "Health").
- a job needing the version job checks the chart's version against the tag,
  packages deploy/helm/oml with `helm package` and signs the archive with
  `cosign sign-blob`, and a job needing both it and the job that creates the
  GitHub release uploads the archive and its signature bundle with
  `gh release upload` (docs/adr/ADR-0033-interim-helm-chart.md d10);
- a matrix job builds `oml` for every CLI target in CLI_TARGETS, each on the
  native runner named there, packages it as oml-cli-${VERSION}-${TARGET}
  (.zip on Windows, .tar.gz elsewhere) after the binary prints the tag's
  version, and a matrix job builds oml-server-lead for every target in
  SERVER_TARGETS as oml-server-lead-${VERSION}-${TARGET}.tar.gz, and one
  builds oml-web, the web console's own process (ADR-0070 d4), for the
  same targets as oml-web-${VERSION}-${TARGET}.tar.gz after the binary
  prints the tag's version;
- one job, needing both and holding `id-token: write`, writes one CycloneDX
  SBOM per archive with `cargo cyclonedx --target`, one SHA256SUMS over every
  archive and SBOM, and only then signs every archive, every SBOM and
  SHA256SUMS with `cosign sign-blob`, and the job that creates the GitHub
  release needs it;
- a job needing the version job runs scripts/check_agent_kit.py and then
  scripts/package_agent_artifacts.py with the tag's version, the repository
  and the tag's commit; the signing job needs it, requires
  oml-claude-plugins-${VERSION} and oml-agent-kit-${VERSION} as .tar.gz and
  .zip, and oml-marketplace-${VERSION}.json, among the files it downloads,
  sums the pinned marketplace in SHA256SUMS beside the archives and signs it
  (docs/engineering/ci-and-toolchain.md "Release artifacts");
- a job needing the version job runs scripts/build_sdk_packages.py and uploads the
  SDK packages, and the signing job needs it and requires open_memory_lab_sdk-${VERSION}-py3-none-any.whl,
  open_memory_lab_sdk-${VERSION}.tar.gz and open-memory-lab-sdk-${VERSION}.tgz among the files it sums and
  signs; the version job runs scripts/check_sdk_versions.py with the tag; the only
  jobs that upload to PyPI or npm are publish-sdk-pypi and publish-sdk-npm, each
  skipped unless vars.PUBLISH_SDKS is 'true', each needing the version job and the
  GitHub release job, each checking the files against SHA256SUMS first and holding
  `id-token: write`; every action in those three jobs is pinned by commit, the publish
  jobs have no step-level `if:`, run the checksum step before the upload (any `npm`,
  `pnpm`, `yarn`, `twine` or `uv` publish, with flags or through npx), give the
  PyPI action `print-hash` and no other input, write the name guard and the
  checksum line whole, fill dist/ in the PyPI job with the two checked files alone
  and check out only the tag (docs/contributing/releasing.md, "Publishing the SDKs");
- ci.yml job cli-matrix runs on the same target and runner pairs and, on each,
  runs `oml --version` and the CLI's unit tests, so a broken platform fails a
  pull request before a tag (docs/engineering/ci-and-toolchain.md "Release
  artifacts");
- both of those jobs name musl-gcc as the C compiler of every musl target, in
  CC_<target> of the job's env: on the aarch64 runner the cc crate counts the
  musl target as a cross compile and would otherwise call
  aarch64-linux-musl-gcc, which Ubuntu's musl-tools does not ship;
- the installers, scripts/install/install.sh and install.ps1, name no release
  file but their own OS's oml-cli archive, SHA256SUMS and SHA256SUMS's
  signature bundle, which the signing job sums or signs; name only CLI targets
  the release builds, and every one of their OS's; verify SHA256SUMS with
  cosign verify-blob under the release workflow's identity; install the
  workspace version by default; and ci.yml job install-smoke runs them on
  Linux and Windows (macOS paused), then the installed `oml --version`
  (docs/adr/ADR-0041-a-quick-start-in-one-command-per-role.md d1, d2, d8);
- every installer URL a reader page shows (README.md, a plugin, SDK or kit
  README, and docs/ outside adr/, atlas/ and changelog.md) fetches
  scripts/install/ from raw.githubusercontent.com at the tag of the workspace
  version, or at the placeholder vX.Y.Z, never at main or another tag, and the
  README's Quickstart is the three lines the v0.4.0 documentation plan names:
  the installer at that tag, `oml login` and `oml setup claude-code`
  (ADR-0041 d4, d6, d7; docs/engineering/docs-upgrade-v0.4.0.md).

- the job that creates the GitHub release runs scripts/release_markers.py append and then check on
  the notes it wrote, before `gh release create`, on a checkout with every tag, so a release whose
  scanner rule set differs from the previous tag's carries the admin-rescan marker line
  (docs/contributing/releasing.md, "Markers in the release notes");

- the version job also runs `python3 scripts/release_markers.py list --tag "$GITHUB_REF_NAME"` on a
  checkout with every tag, so a scanner source whose rule set that script cannot read stops the
  release before anything is built, pushed or signed (#334);

- no release build turns on oml-server-lead's e2e-sign-in feature, CI's
  sign-in double: release.yml names neither it nor OML_CARGO_FEATURES, and
  the Dockerfile's OML_CARGO_FEATURES defaults to empty (ADR-0070, phase (b)
  as built; #242).

Dependencies: stdlib (Python 3.11 or later, for tomllib) + PyYAML.
"""

import re
import sys
import tomllib
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github/workflows/release.yml"
CI = ROOT / ".github/workflows/ci.yml"
CARGO = ROOT / "Cargo.toml"
# Each CLI target and the GitHub-hosted runner of its own OS and architecture
# (docs/engineering/ci-and-toolchain.md "Release artifacts").
CLI_TARGETS = {
    "x86_64-unknown-linux-musl": "ubuntu-24.04",
    "aarch64-unknown-linux-musl": "ubuntu-24.04-arm",
    "x86_64-pc-windows-msvc": "windows-2025",
}
SERVER_TARGETS = {"x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"}
# The compiler each musl CLI target's C (ring's) is built with: the wrapper
# Ubuntu's musl-tools ships for the runner's own architecture. cc 1.10.0 maps
# aarch64-unknown-linux-musl to the fixed prefix aarch64-linux-musl when the
# host is aarch64-unknown-linux-gnu, a binary musl-tools does not ship
# (packages.ubuntu.com/noble/arm64/musl-tools/filelist, fetched 2026-11-03).
MUSL_CC = {
    f"CC_{target.replace('-', '_')}": "musl-gcc" for target in CLI_TARGETS if target.endswith("-linux-musl")
}
CLI_ARCHIVE = "oml-cli-${VERSION}-${TARGET}"
SERVER_ARCHIVE = "oml-server-lead-${VERSION}-${TARGET}"
WEB_ARCHIVE = "oml-web-${VERSION}-${TARGET}"
CARGO_BUILD = re.compile(r"\bcargo\s+build\b[^\n]*--bin\s+(\S+)")
CYCLONEDX = re.compile(r"\bcargo\s+cyclonedx\b[^\n]*--target\b")
SHA256SUMS = re.compile(
    r"\bsha256sum\b[^\n]*\*\.tar\.gz[^\n]*\*\.zip[^\n]*\*\.whl[^\n]*\*\.tgz[^\n]*\*\.cdx\.json[^\n]*oml-marketplace-\*\.json[^\n]*>\s*SHA256SUMS"
)
SIGN_EVERY_BLOB = re.compile(
    r"for\s+f\s+in\s+\*\.tar\.gz\s+\*\.zip\s+\*\.whl\s+\*\.tgz\s+\*\.cdx\.json\s+oml-marketplace-\*\.json\s+SHA256SUMS;"
    r"[^\n]*\n[^\n]*cosign\s+sign-blob\b"
)
PACKAGE_AGENTS = re.compile(r"\bscripts/package_agent_artifacts\.py\b")
CHECK_AGENTS = re.compile(r"\bscripts/check_agent_kit\.py\b")
# The agent artifacts every release carries (scripts/package_agent_artifacts.py).
AGENT_FILES = (
    "oml-claude-plugins-${VERSION}.tar.gz",
    "oml-claude-plugins-${VERSION}.zip",
    "oml-agent-kit-${VERSION}.tar.gz",
    "oml-agent-kit-${VERSION}.zip",
    "oml-marketplace-${VERSION}.json",
)
PLATFORMS = {"linux/amd64", "linux/arm64"}
BUILD_ACTION = "docker/build-push-action@"
MATRIX_REF = re.compile(r"^\$\{\{\s*matrix\.([A-Za-z_][\w-]*)\s*\}\}$")
COSIGN_SIGN = re.compile(r"\bcosign\s+sign(?=\s)")
BUILD_ARGS = ("OML_BUILD_COMMIT", "OML_BUILD_COMMIT_TIME")
HELM_PACKAGE = re.compile(r"\bhelm\s+package\s+deploy/helm/oml\b")
CHART_VERSION = re.compile(r"\bhelm\s+show\s+chart\s+deploy/helm/oml\b")
COSIGN_SIGN_BLOB = re.compile(r"\bcosign\s+sign-blob\b[^\n]*\.tgz")
RELEASE_CREATE = re.compile(r"\bgh\s+release\s+create\b")
RELEASE_UPLOAD = re.compile(r"\bgh\s+release\s+upload\b")
REPOSITORY = "e8kor/open-memory-lab"
# Each installer, the CLI targets of its OS, and the release files it may
# fetch, with its variables written {version} and {target}: its own archive,
# SHA256SUMS, which the signing job writes over every archive, and
# SHA256SUMS's cosign bundle (ADR-0041 d2).
INSTALLERS = {
    "scripts/install/install.sh": (
        {t for t in CLI_TARGETS if not t.endswith("-windows-msvc")},
        {"oml-cli-{version}-{target}.tar.gz", "SHA256SUMS", "SHA256SUMS.sigstore.json"},
    ),
    "scripts/install/install.ps1": (
        {t for t in CLI_TARGETS if t.endswith("-windows-msvc")},
        {"oml-cli-{version}-{target}.zip", "SHA256SUMS", "SHA256SUMS.sigstore.json"},
    ),
}
# A release file an installer names: an archive, a bundle, an SBOM or SHA256SUMS.
RELEASE_FILE = re.compile(r"[\w${}.-]*(?:\.tar\.gz|\.zip|\.sigstore\.json|\.cdx\.json)\b|\bSHA256SUMS[\w.-]*")
SHELL_VAR = re.compile(r"\$\{?([A-Za-z_]\w*)\}?")
TARGET_TRIPLE = re.compile(r"\b(?:x86_64|aarch64|i686|armv7|arm64)-(?:unknown|apple|pc)-[a-z]+(?:-[a-z]+)?\b")
DEFAULT_VERSION = re.compile(r"(?:DEFAULT_VERSION=|\$DefaultVersion\s*=\s*)[\"']([^\"']+)[\"']")


def main() -> int:
    errors: list[str] = []
    jobs = yaml.safe_load(WORKFLOW.read_text())["jobs"]

    builds = {name: job for name, job in jobs.items() if build_steps(job)}
    if not builds:
        errors.append(f"no job runs {BUILD_ACTION}")
        return report(errors)

    built: set[str] = set()
    for name, job in builds.items():
        for step in build_steps(job):
            given = step.get("with", {})
            built |= platforms_of(job, given.get("platforms", ""))
            if "push-by-digest=true" not in str(given.get("outputs", "")):
                errors.append(f"job {name}: the image build does not push by digest")
            build_args = str(given.get("build-args", ""))
            for arg in BUILD_ARGS:
                if f"{arg}=" not in build_args:
                    errors.append(f"job {name}: the image build does not pass {arg}, so /readyz would report it unknown")
        if any(COSIGN_SIGN.search(run) for run in runs(job)):
            errors.append(f"job {name}: signs a single-platform digest, not the index")
    for platform in sorted(PLATFORMS - built):
        errors.append(f"the image is built for {sorted(built)}, never for {platform}")

    merges = {
        name: job
        for name, job in jobs.items()
        if set(needs(job)) & set(builds) and step_index(job, "docker buildx imagetools create") is not None
    }
    if not merges:
        errors.append("no job needing the image build assembles the index with docker buildx imagetools create")

    for name, job in merges.items():
        create = step_index(job, "docker buildx imagetools create")
        inspect = step_index(job, "docker buildx imagetools inspect --raw")
        sign = next(
            (i for i, run in enumerate(runs(job)) if COSIGN_SIGN.search(run) and "--recursive" in run),
            None,
        )
        if inspect is None or inspect < create:
            errors.append(f"job {name}: does not assert the index's platforms after assembling it")
        if sign is None:
            errors.append(f"job {name}: does not sign the index with cosign sign --recursive")
        elif inspect is not None and sign < inspect:
            errors.append(f"job {name}: signs the index before asserting its platforms")

    check_release_hold(errors, jobs)
    check_chart(errors, jobs)
    check_binaries(errors, jobs)
    check_agents(errors, jobs)
    check_sdks(errors, jobs)
    check_cli_matrix(errors, jobs)
    check_installers(errors, jobs)
    check_installer_lines(errors)
    check_release_markers(errors, jobs)
    check_early_markers(errors, jobs)
    check_no_test_double(errors, WORKFLOW.read_text(), (ROOT / "Dockerfile").read_text())
    return report(errors)


def check_release_markers(errors: list[str], jobs: dict) -> None:
    """The job that creates the GitHub release appends the markers an installer may read to the
    notes it wrote, and checks them, before `gh release create`, on a checkout that holds every tag
    (scripts/release_markers.py): a release whose scanner rule set differs from the previous tag's
    carries the admin-rescan marker, and a release without it does not go out."""
    creators = {name: job for name, job in jobs.items() if step_matching(job, RELEASE_CREATE) is not None}
    for name, job in creators.items():
        create = step_matching(job, RELEASE_CREATE)
        found = {}
        for action in ("append", "check"):
            found[action] = marker_command(job, action)
            if found[action] is None:
                errors.append(f"job {name}: does not run scripts/release_markers.py {action} on the release notes")
            elif found[action][0] >= create:
                errors.append(f"job {name}: runs scripts/release_markers.py {action} at or after the step that creates the release, not before it")
        if None not in found.values() and found["check"] < found["append"]:
            errors.append(f"job {name}: checks the release markers before it appends them")
        deep = [s for s in steps(job) if str(s.get("uses", "")).startswith("actions/checkout@")]
        if not any(str(s.get("with", {}).get("fetch-depth")) == "0" for s in deep):
            errors.append(f"job {name}: checks out without fetch-depth: 0, so there are no tags to compare the scanner's rule set with")


def check_early_markers(errors: list[str], jobs: dict) -> None:
    """The version job, which every other job needs, runs `python3 scripts/release_markers.py list`
    on a checkout that holds every tag, so a scanner source the script cannot read fails the release
    before anything is built, pushed or signed rather than in the publish job after."""
    version = jobs.get("version")
    if version is None:
        return  # check_release_hold names the missing job
    if marker_command(version, "list") is None:
        errors.append("the version job does not run scripts/release_markers.py list, so an unparsable scanner rule-set constant would fail the release only after the images are pushed")
    deep = [s for s in steps(version) if str(s.get("uses", "")).startswith("actions/checkout@")]
    if not any(str(s.get("with", {}).get("fetch-depth")) == "0" for s in deep):
        errors.append("the version job checks out without fetch-depth: 0, so scripts/release_markers.py list has no tags to read")


def marker_command(job: dict, action: str) -> tuple[int, int] | None:
    """Where a step's script runs `python3 scripts/release_markers.py <action>` as a command line,
    as (step index, line index), or None. A line that only mentions the script, as an `echo` or a
    comment does, is not a run of it."""
    command = re.compile(rf"^python3\s+scripts/release_markers\.py\s+{action}\b")
    for i, run in enumerate(runs(job)):
        for j, line in enumerate(run.splitlines()):
            if command.match(line.strip()):
                return (i, j)
    return None


def check_no_test_double(errors: list[str], workflow: str, dockerfile: str) -> None:
    """The sign-in double CI's signed-in page check builds never reaches a release: release.yml
    names neither the feature nor the image's OML_CARGO_FEATURES build argument, whose value a
    workflow variable could carry past a check of the name (#242), and the Dockerfile defaults
    that argument to empty."""
    if "e2e-sign-in" in workflow:
        errors.append("release.yml names the e2e-sign-in feature, CI's sign-in double; no release carries it")
    if "OML_CARGO_FEATURES" in workflow:
        errors.append("release.yml sets OML_CARGO_FEATURES; a release image builds with the Dockerfile's empty default")
    default = re.search(r'^ARG OML_CARGO_FEATURES(?:=("?)([^"\n]*)\1)?\s*$', dockerfile, re.M)
    if default is None:
        errors.append("the Dockerfile declares no ARG OML_CARGO_FEATURES; the image's features must default to none")
    elif default.group(2):
        errors.append(f"the Dockerfile's OML_CARGO_FEATURES defaults to {default.group(2)!r}, not empty")


def check_release_hold(errors: list[str], jobs: dict) -> None:
    """The version job runs scripts/check_release_hold.py before anything else, and every other
    job needs it, so RELEASE-HOLD stops the whole release."""
    version = jobs.get("version")
    if version is None:
        errors.append("no version job, so nothing runs scripts/check_release_hold.py first")
        return
    hold = step_index(version, "scripts/check_release_hold.py")
    if hold is None:
        errors.append("the version job does not run scripts/check_release_hold.py")
    elif hold != next(i for i, run in enumerate(runs(version)) if run):
        errors.append("the version job runs another step before scripts/check_release_hold.py")
    for name in jobs:
        if name != "version" and "version" not in ancestors(jobs, name):
            errors.append(f"job {name}: does not need the version job, so RELEASE-HOLD would not stop it")


def matrix_rows(job: dict) -> list[dict]:
    return list(job.get("strategy", {}).get("matrix", {}).get("include", []))


def builders(jobs: dict, binary: str) -> dict:
    """The jobs that run `cargo build --bin <binary>`."""
    return {
        name: job
        for name, job in jobs.items()
        if any(binary in CARGO_BUILD.findall(run) for run in runs(job))
    }


def check_binaries(errors: list[str], jobs: dict) -> None:
    """Every CLI and server target is built natively, packaged, given an SBOM, summed and signed."""
    cli = builders(jobs, "oml")
    server = builders(jobs, "oml-server-lead")
    web = builders(jobs, "oml-web")
    for binary, found in (("oml", cli), ("oml-server-lead", server), ("oml-web", web)):
        if len(found) != 1:
            errors.append(f"{len(found)} jobs build --bin {binary}, not exactly one")
    for name, job in cli.items():
        rows = {str(r.get("target")): str(r.get("runner")) for r in matrix_rows(job)}
        if rows != CLI_TARGETS:
            errors.append(f"job {name}: builds the CLI for {rows}, not {CLI_TARGETS}")
        script = "\n".join(runs(job))
        if job.get("runs-on") != "${{ matrix.runner }}":
            errors.append(f"job {name}: does not run each target on its own runner")
        check_musl_cc(errors, f"job {name}", job)
        for needle, why in (
            (CLI_ARCHIVE, "does not name its archive oml-cli-${VERSION}-${TARGET}"),
            ('.zip"', "does not package Windows as a .zip"),
            ('.tar.gz"', "does not package the Unix targets as a .tar.gz"),
            ("--version", "does not run the built oml --version before packaging it"),
        ):
            if needle not in script:
                errors.append(f"job {name}: {why}")
    for name, job in server.items():
        targets = {str(r.get("target")) for r in matrix_rows(job)}
        if targets != SERVER_TARGETS:
            errors.append(f"job {name}: builds the server for {sorted(targets)}, not {sorted(SERVER_TARGETS)}")
        script = "\n".join(runs(job))
        if SERVER_ARCHIVE not in script or '.tar.gz"' not in script:
            errors.append(f"job {name}: does not package {SERVER_ARCHIVE}.tar.gz")
    for name, job in web.items():
        targets = {str(r.get("target")) for r in matrix_rows(job)}
        if targets != SERVER_TARGETS:
            errors.append(f"job {name}: builds oml-web for {sorted(targets)}, not {sorted(SERVER_TARGETS)}")
        script = "\n".join(runs(job))
        if WEB_ARCHIVE not in script or '.tar.gz"' not in script:
            errors.append(f"job {name}: does not package {WEB_ARCHIVE}.tar.gz")
        if "--version" not in script:
            errors.append(f"job {name}: does not run the built oml-web --version before packaging it")

    signers = {
        name: job
        for name, job in jobs.items()
        if any(SIGN_EVERY_BLOB.search(run) for run in runs(job))
    }
    if len(signers) != 1:
        errors.append(
            f"{len(signers)} jobs sign every archive, SBOM and SHA256SUMS with cosign sign-blob, not exactly one"
        )
    for name, job in signers.items():
        upstream = ancestors(jobs, name)
        if not set(cli) <= upstream or not set(server) <= upstream or not set(web) <= upstream:
            errors.append(f"job {name}: signs without needing every job that builds an archive")
        if job.get("permissions", {}).get("id-token") != "write":
            errors.append(f"job {name}: has no id-token: write, so keyless signing has no identity")
        env = job.get("env", {})
        if set(str(env.get("CLI_TARGETS", "")).split()) != set(CLI_TARGETS):
            errors.append(f"job {name}: CLI_TARGETS is not {sorted(CLI_TARGETS)}")
        if set(str(env.get("SERVER_TARGETS", "")).split()) != SERVER_TARGETS:
            errors.append(f"job {name}: SERVER_TARGETS is not {sorted(SERVER_TARGETS)}")
        if set(str(env.get("WEB_TARGETS", "")).split()) != SERVER_TARGETS:
            errors.append(f"job {name}: WEB_TARGETS is not {sorted(SERVER_TARGETS)}")
        sbom = step_matching(job, CYCLONEDX)
        sums = step_matching(job, SHA256SUMS)
        sign = step_matching(job, SIGN_EVERY_BLOB)
        if sbom is None:
            errors.append(f"job {name}: writes no per-target SBOM with cargo cyclonedx --target")
        if sums is None:
            errors.append(f"job {name}: writes no SHA256SUMS over every archive and SBOM")
        elif sbom is not None and sums < sbom:
            errors.append(f"job {name}: sums the archives before writing their SBOMs")
        if sign is not None and sums is not None and sign < sums:
            errors.append(f"job {name}: signs before SHA256SUMS is written")
        script = "\n".join(runs(job))
        for stem in (
            "oml-cli-${VERSION}-$t.cdx.json",
            "oml-server-lead-${VERSION}-$t.cdx.json",
            "oml-web-${VERSION}-$t.cdx.json",
        ):
            if stem not in script:
                errors.append(f"job {name}: writes no SBOM named {stem}")
        creators = [n for n, j in jobs.items() if step_matching(j, RELEASE_CREATE) is not None]
        for creator in creators:
            if name not in ancestors(jobs, creator):
                errors.append(f"job {creator}: creates the release without needing job {name}")


def check_agents(errors: list[str], jobs: dict) -> None:
    """One job packages the agent artifacts after their check, and the signing job requires, sums and signs them."""
    versions = {name for name, job in jobs.items() if "version" in job.get("outputs", {})}
    packagers = {name: job for name, job in jobs.items() if step_matching(job, PACKAGE_AGENTS) is not None}
    if len(packagers) != 1:
        errors.append(f"{len(packagers)} jobs run scripts/package_agent_artifacts.py, not exactly one")
    for name, job in packagers.items():
        run = runs(job)[step_matching(job, PACKAGE_AGENTS)]
        check = step_matching(job, CHECK_AGENTS)
        if not set(needs(job)) & versions:
            errors.append(f"job {name}: packages the agent artifacts without needing the job that reads the tag's version")
        if check is None or check > step_matching(job, PACKAGE_AGENTS):
            errors.append(f"job {name}: does not run scripts/check_agent_kit.py before packaging")
        for flag, value in (("--version", '"$VERSION"'), ("--repo", '"$GITHUB_REPOSITORY"'), ("--sha", '"$GITHUB_SHA"')):
            if f"{flag} {value}" not in run:
                errors.append(f"job {name}: does not pass {flag} {value} to the packaging script")
    signers = {name: job for name, job in jobs.items() if any(SIGN_EVERY_BLOB.search(r) for r in runs(job))}
    for name, job in signers.items():
        if not set(packagers) <= ancestors(jobs, name):
            errors.append(f"job {name}: signs without needing the job that packages the agent artifacts")
        script = "\n".join(runs(job))
        for want in AGENT_FILES:
            if want not in script:
                errors.append(f"job {name}: does not require {want} among the files it signs")


def check_cli_matrix(errors: list[str], jobs: dict) -> None:
    """ci.yml builds and tests the CLI on every release target's runner before a tag."""
    ci = yaml.safe_load(CI.read_text())["jobs"]
    job = ci.get("cli-matrix")
    if job is None:
        errors.append("ci.yml has no job cli-matrix, so a broken CLI platform is caught only by a tag")
        return
    rows = {str(r.get("target")): str(r.get("runner")) for r in matrix_rows(job)}
    released = {
        str(r.get("target")): str(r.get("runner")) for j in builders(jobs, "oml").values() for r in matrix_rows(j)
    }
    if rows != released:
        errors.append(f"ci.yml cli-matrix runs {rows}, but release.yml builds the CLI on {released}")
    check_musl_cc(errors, "ci.yml cli-matrix", job)
    script = "\n".join(runs(job))
    if "--version" not in script:
        errors.append("ci.yml cli-matrix does not run the built oml --version")
    if not re.search(r"\bcargo\s+test\b[^\n]*-p\s+oml-cli\b[^\n]*--target", script):
        errors.append("ci.yml cli-matrix does not run the CLI's unit tests for its target")


def check_installers(errors: list[str], jobs: dict) -> None:
    """The installers fetch only signed release files, verify them under the release identity, and CI runs them."""
    identities = [m.group(1) for j in jobs.values() for run in runs(j) if (m := re.search(r"identity='([^']+)'", run))]
    if not identities:
        errors.append("release.yml names no signing identity for the installers to verify against")
        return
    # The release identity up to its tag, with the repository filled in.
    prefix = identities[0].replace("${{ github.repository }}", REPOSITORY).split("refs/tags/")[0] + "refs/tags/"
    workspace = tomllib.loads(CARGO.read_text())["workspace"]["package"]["version"]
    for path, (targets, allowed) in INSTALLERS.items():
        file = ROOT / path
        if not file.exists():
            errors.append(f"{path} does not exist")
            continue
        text = file.read_text()
        named = {SHELL_VAR.sub(lambda m: "{" + m.group(1).lower() + "}", f.rstrip(".")) for f in RELEASE_FILE.findall(text)}
        for name in sorted(named - allowed):
            errors.append(f"{path}: names {name}, which is not its OS's CLI archive, SHA256SUMS or SHA256SUMS's bundle")
        for name in sorted(allowed - named):
            errors.append(f"{path}: never names {name}, so it cannot check the archive against the signed SHA256SUMS")
        triples = set(TARGET_TRIPLE.findall(text))
        for t in sorted(triples - set(CLI_TARGETS)):
            errors.append(f"{path}: names target {t}, which the release does not build")
        for t in sorted(targets - triples):
            errors.append(f"{path}: never names target {t}, so that platform gets no install")
        if "cosign verify-blob" not in text:
            errors.append(f"{path}: never runs cosign verify-blob")
        # install.sh writes the identity in double quotes, so each backslash is doubled there.
        if "token.actions.githubusercontent.com" not in text or prefix not in text.replace("\\\\", "\\"):
            errors.append(f"{path}: does not verify against the release workflow's identity {prefix}<tag>")
        default = DEFAULT_VERSION.search(text)
        if not default:
            errors.append(f"{path}: names no default version")
        elif default.group(1) != workspace:
            errors.append(f"{path}: installs {default.group(1)} by default, not the workspace version {workspace}")

    job = yaml.safe_load(CI.read_text())["jobs"].get("install-smoke")
    if job is None:
        errors.append("ci.yml has no job install-smoke, so no installer has run against a release")
        return
    runners = {str(r.get("runner")) for r in matrix_rows(job)}
    for os_name in ("ubuntu", "windows"):  # macOS paused (ci-and-toolchain.md "Paused targets")
        if not any(r.startswith(os_name) for r in runners):
            errors.append(f"ci.yml install-smoke has no {os_name} runner")
    script = "\n".join(runs(job))
    for needle in ("scripts/install/install.sh", "scripts/install/install.ps1", "--version"):
        if needle not in script:
            errors.append(f"ci.yml install-smoke never runs {needle}")


# An installer URL on a reader page, and the tag it fetches the script at.
INSTALLER_URL = re.compile(
    r"https://raw\.githubusercontent\.com/" + re.escape(REPOSITORY) + r"/([^/\s]+)/scripts/install/install\.(?:sh|ps1)"
)
INSTALLER_LINE_EXEMPT = [ROOT / "docs" / "adr", ROOT / "docs" / "atlas", ROOT / "docs" / "changelog.md"]


def reader_pages() -> list[Path]:
    pages = [ROOT / "README.md", *sorted(ROOT.glob("plugins/*/README.md")), *sorted(ROOT.glob("sdks/*/README.md")),
             ROOT / "kit" / "README.md", *sorted((ROOT / "docs").rglob("*.md"))]
    return [p for p in pages if p.exists() and not any(p == e or e in p.parents for e in INSTALLER_LINE_EXEMPT)]


def check_installer_lines(errors: list[str]) -> None:
    """Each documented installer line fetches the script at the workspace version's tag, and the
    README's Quickstart is the installer, oml login and oml setup claude-code (ADR-0041 d4, d6, d7)."""
    workspace = tomllib.loads(CARGO.read_text())["workspace"]["package"]["version"]
    tags = {f"v{workspace}", "vX.Y.Z"}
    for page in reader_pages():
        for number, line in enumerate(page.read_text(encoding="utf-8").splitlines(), 1):
            for tag in INSTALLER_URL.findall(line):
                if tag not in tags:
                    errors.append(
                        f"{page.relative_to(ROOT)}:{number}: the installer is fetched at {tag}, not at "
                        f"v{workspace} or vX.Y.Z (ADR-0041 d4)"
                    )
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    section = re.search(r"^## Quickstart\n(.*?)(?=^## )", readme, re.M | re.S)
    if not section:
        errors.append("README.md has no Quickstart section")
        return
    blocks = re.findall(r"^```(?:sh|bash|console)\n(.*?)^```", section.group(1), re.M | re.S)
    if not blocks:
        errors.append("README.md's Quickstart shows no shell block")
        return
    lines = [ln.split("  #")[0].strip() for ln in blocks[0].replace("\\\n", " ").splitlines()]
    lines = [ln for ln in lines if ln and not ln.startswith("#")]
    wanted = [
        ("the installer", lambda ln: INSTALLER_URL.search(ln) and f"/v{workspace}/" in ln and ln.endswith("| sh")),
        ("oml login", lambda ln: ln.startswith("oml login ")),
        ("oml setup claude-code", lambda ln: ln.startswith("oml setup claude-code")),
    ]
    if len(lines) != len(wanted) or not all(ok(ln) for (_, ok), ln in zip(wanted, lines)):
        errors.append(
            "README.md's Quickstart opens with " + repr(lines) + ", not exactly three lines: "
            + ", ".join(name for name, _ in wanted) + f", the installer at v{workspace} (ADR-0041 d4, d6, d7)"
        )


def check_musl_cc(errors: list[str], where: str, job: dict) -> None:
    """The job's env names musl-gcc as the C compiler of every musl CLI target."""
    env = job.get("env", {})
    for var, want in MUSL_CC.items():
        if str(env.get(var, "")) != want:
            errors.append(f"{where}: env {var} is {env.get(var)!r}, not {want!r}, so cc may call a compiler musl-tools lacks")


def check_chart(errors: list[str], jobs: dict) -> None:
    """The chart is packaged at the tag's version, signed, and attached to the release (ADR-0033 d10)."""
    versions = {name for name, job in jobs.items() if "version" in job.get("outputs", {})}
    packagers = {name: job for name, job in jobs.items() if step_matching(job, HELM_PACKAGE) is not None}
    if not packagers:
        errors.append("no job packages deploy/helm/oml with helm package, so the release carries no chart")
        return
    for name, job in packagers.items():
        package = step_matching(job, HELM_PACKAGE)
        compare = step_matching(job, CHART_VERSION)
        sign = step_matching(job, COSIGN_SIGN_BLOB)
        if not set(needs(job)) & versions:
            errors.append(f"job {name}: packages the chart without needing the job that reads the tag's version")
        if compare is None or compare > package:
            errors.append(f"job {name}: does not check the chart's version against the tag before packaging it")
        if sign is None or sign < package:
            errors.append(f"job {name}: does not sign the chart archive with cosign sign-blob after packaging it")

    creators = {name for name, job in jobs.items() if step_matching(job, RELEASE_CREATE) is not None}
    uploaders = {
        name: job
        for name, job in jobs.items()
        if any(RELEASE_UPLOAD.search(run) and ".tgz" in run for run in runs(job))
    }
    if not uploaders:
        errors.append("no job uploads the chart archive to the GitHub release with gh release upload")
    for name, job in uploaders.items():
        upstream = ancestors(jobs, name)
        if not upstream & creators:
            errors.append(f"job {name}: uploads the chart without needing the job that creates the release")
        if not upstream & set(packagers):
            errors.append(f"job {name}: uploads the chart without needing the job that packages it")
        if not any(RELEASE_UPLOAD.search(run) and ".sigstore.json" in run for run in runs(job)):
            errors.append(f"job {name}: uploads the chart archive without its signature bundle")


# The file names come from scripts/sdk_names.py through the version job's outputs, as env vars.
SDK_FILES = ("SDK_WHEEL", "SDK_SDIST", "SDK_TGZ")
# Each publish job, the environment it must keep and the files it uploads.
SDK_PUBLISHERS = {
    "publish-sdk-pypi": ("pypi", ("SDK_WHEEL", "SDK_SDIST")),
    "publish-sdk-npm": ("npm", ("SDK_TGZ",)),
}
# `npm publish`, `npm --tag next publish`, `npx npm publish`, `pnpm publish`, `yarn npm publish`: the tool, then
# `publish` after any flags on the same command.
NPM_PUBLISH = re.compile(r"\b(?:npm|pnpm|yarn)\b[^\n;&|]*?\bpublish\b")
PUBLISH_FROM_A_PATH = re.compile(r'\bpublish\s+"?\./')
# A publish job's checkout is the tag that triggered the release, whichever way it is written.
TAG_REFS = {"${{github.ref_name}}", "${{github.ref}}", "refs/tags/${{github.ref_name}}"}
# The one input a publish job may give the PyPI action. Every other one is refused, by whichever of its
# spellings (the action reads `packages_dir` as it reads `packages-dir`): it can point the action at other
# files, another index or a stored credential, or change what a rerun or an attestation does.
PYPI_INPUTS_ALLOWED = ("print-hash",)
# The two commands a publish job stops on, as each must be written, whole: a `||`, a `&`, a pipe after
# it, an `if` or an `echo` around it would let the job carry on past a failure.
NAME_GUARD = "python3 scripts/check_sdk_registry_names.py"
CHECKSUM_LINE = "printf '%s\\n' \"$line\" | sha256sum --check --strict -"
# How the PyPI job fills dist/, the directory the action uploads: these two lines and nothing else.
PYPI_DIST_LINES = ("mkdir dist", 'cp "release/$SDK_WHEEL" "release/$SDK_SDIST" dist/')
MENTIONS_DIST = re.compile(r"(?<![\w./-])dist(?![\w.-])")
OR_TRUE = re.compile(r"\|\|\s*true\b")
PINNED_BY_COMMIT = re.compile(r"^[\w.-]+/[\w./-]+@[0-9a-f]{40}(\s|$)")
UPLOADS_TO_A_REGISTRY = re.compile(NPM_PUBLISH.pattern + r"|\btwine\s+upload\b|\buv\s+publish\b")


def check_sdks(errors: list[str], jobs: dict) -> None:
    """The SDK packages are built at the tag's version and signed with everything else; publishing
    them is two gated jobs and nothing else (docs/contributing/releasing.md, "Publishing the SDKs")."""
    versions = {name for name, job in jobs.items() if "version" in job.get("outputs", {})}
    builders = {
        name: job for name, job in jobs.items()
        if any("scripts/build_sdk_packages.py" in r for r in runs(job))
    }
    if len(builders) != 1:
        errors.append(f"{len(builders)} jobs run scripts/build_sdk_packages.py, not exactly one")
    for name, job in builders.items():
        if not set(needs(job)) & versions:
            errors.append(f"job {name}: builds the SDK packages without needing the job that reads the tag's version")
        if any(UPLOADS_TO_A_REGISTRY.search(r) or "gh-action-pypi-publish" in str(s.get("uses", ""))
               for r, s in zip(runs(job), steps(job))):
            errors.append(f"job {name}: builds the SDK packages and uploads them to a registry")
    for name in versions:
        script = "\n".join(runs(jobs[name]))
        if "scripts/check_sdk_versions.py" not in script or '--tag "$tag"' not in script:
            errors.append(f"job {name}: does not run scripts/check_sdk_versions.py with the tag")
        if "scripts/sdk_names.py" not in script:
            errors.append(f"job {name}: does not write the SDK file names with scripts/sdk_names.py")
    signers = {name: job for name, job in jobs.items() if any(SIGN_EVERY_BLOB.search(r) for r in runs(job))}
    for name, job in signers.items():
        if not set(builders) <= ancestors(jobs, name):
            errors.append(f"job {name}: signs without needing the job that builds the SDK packages")
        script = "\n".join(runs(job))
        for file in SDK_FILES:
            if f"${file}" not in script:
                errors.append(f"job {name}: does not require ${file} among the files it signs")
            if job.get("env", {}).get(file) != f"${{{{ needs.version.outputs.{file.lower()} }}}}":
                errors.append(f"job {name}: {file} is not the version job's {file.lower()} output")
    for name, job in jobs.items():
        uploads = any(
            UPLOADS_TO_A_REGISTRY.search(r) or "gh-action-pypi-publish" in str(s.get("uses", ""))
            for r, s in zip(runs(job), steps(job))
        )
        if uploads and name not in SDK_PUBLISHERS:
            errors.append(f"job {name}: uploads to PyPI or npm; only {sorted(SDK_PUBLISHERS)} may")
    release_creators = [n for n, j in jobs.items() if step_matching(j, RELEASE_CREATE) is not None]
    for name, (environment, files) in sorted(SDK_PUBLISHERS.items()):
        job = jobs.get(name)
        if job is None:
            errors.append(f"no job {name}")
            continue
        if str(job.get("if", "")).replace(" ", "") != "vars.PUBLISH_SDKS=='true'":
            errors.append(f"job {name}: is not gated on vars.PUBLISH_SDKS == 'true', so a release would publish by surprise")
        if job.get("environment") != environment:
            errors.append(f"job {name}: does not run in the {environment!r} environment, which holds the registry's trust and the reviewer")
        upstream = ancestors(jobs, name)
        if not versions <= upstream or not set(release_creators) <= upstream:
            errors.append(f"job {name}: does not need the version job and the job that creates the GitHub release")
        script = "\n".join(runs(job))
        guard = step_index(job, "scripts/check_sdk_registry_names.py")
        upload = next(
            (i for i, st in enumerate(steps(job))
             if "gh-action-pypi-publish" in str(st.get("uses", "")) or NPM_PUBLISH.search(str(st.get("run", "")))),
            None,
        )
        checksum = step_index(job, "sha256sum --check --strict -")
        if checksum is not None and upload is not None and checksum >= upload:
            errors.append(f"job {name}: checks the files against SHA256SUMS after the upload")
        if any("if" in st for st in steps(job)):
            errors.append(f"job {name}: has a step-level if:, which can skip a check and still reach the upload")
        for st in steps(job):
            uses, given = str(st.get("uses", "")), st.get("with") or {}
            if "gh-action-pypi-publish" in uses:
                for key in given:
                    if key not in PYPI_INPUTS_ALLOWED:
                        errors.append(f"job {name}: sets {key} on the PyPI action, which may take print-hash alone: any other input can point it away from the checked files, the trusted index or the OIDC identity")
            if uses.startswith("actions/checkout@") and "ref" in given:
                if str(given["ref"]).replace(" ", "") not in TAG_REFS:
                    errors.append(f"job {name}: checks out {given['ref']!r}, not the tag that triggered the release")
        if guard is None:
            errors.append(f"job {name}: does not run scripts/check_sdk_registry_names.py, so an undecided registry name could publish")
        elif upload is not None and guard >= upload:
            errors.append(f"job {name}: runs scripts/check_sdk_registry_names.py after the upload")
        if guard is not None and any(runs(job)[i] for i in range(guard)):
            errors.append(f"job {name}: runs another command before scripts/check_sdk_registry_names.py")
        if guard is not None and runs(job)[guard].strip() != NAME_GUARD:
            errors.append(f"job {name}: the registry-name step is not exactly `{NAME_GUARD}`, so its failure might not stop the job")
        if any("sha256sum --check --strict -" in line and line.strip() != CHECKSUM_LINE for line in script.splitlines()):
            errors.append(f"job {name}: a line that runs sha256sum --check --strict - is not exactly `{CHECKSUM_LINE}`, so a mismatch might not stop the job")
        if name == "publish-sdk-pypi":
            filled = [line.strip() for line in script.splitlines() if MENTIONS_DIST.search(line)]
            if sorted(filled) != sorted(PYPI_DIST_LINES):
                errors.append(
                    f"job {name}: fills dist/ other than by `{PYPI_DIST_LINES[0]}` and `{PYPI_DIST_LINES[1]}`, once each "
                    f"({'; '.join(filled) or 'no such line'}): the action uploads every file in it"
                )
            if checksum is not None and upload is not None and upload - checksum > 1:
                errors.append(f"job {name}: has a step between the checksum step and the upload, which can add files to dist/")
        if "--ignore-missing" in script:
            errors.append(f"job {name}: checks with --ignore-missing, which passes a file SHA256SUMS does not list")
        if "sha256sum --check --strict -" not in script or "awk" not in script:
            errors.append(f"job {name}: does not pull each file's own line out of SHA256SUMS and check it with sha256sum --check --strict -")
        if "set -euo pipefail" not in script:
            errors.append(f"job {name}: the checksum step does not run under set -euo pipefail")
        for file in files:
            if f'"${file}"' not in script:
                errors.append(f"job {name}: does not check ${file} by name against SHA256SUMS")
        if job.get("continue-on-error") or any(st.get("continue-on-error") for st in steps(job)):
            errors.append(f"job {name}: has continue-on-error, so a failed check would not stop the upload")
        if OR_TRUE.search(script):
            errors.append(f"job {name}: has a command ending in || true, which hides a failed check")
        if name == "publish-sdk-npm":
            for run in runs(job):
                if NPM_PUBLISH.search(run) and not PUBLISH_FROM_A_PATH.search(run):
                    errors.append(f"job {name}: npm publish does not start its argument with ./, so npm reads it as a GitHub repository")
    for name in sorted(set(SDK_PUBLISHERS) | set(builders)):
        for step in steps(jobs.get(name, {})):
            uses = str(step.get("uses", ""))
            if uses and not PINNED_BY_COMMIT.match(uses):
                errors.append(f"job {name}: {uses} is not pinned by commit")


def step_matching(job: dict, pattern: re.Pattern) -> int | None:
    return next((i for i, run in enumerate(runs(job)) if pattern.search(run)), None)


def ancestors(jobs: dict, name: str) -> set[str]:
    """Every job `name` needs, directly or through another job."""
    found: set[str] = set()
    todo = list(needs(jobs.get(name, {})))
    while todo:
        job = todo.pop()
        if job not in found:
            found.add(job)
            todo.extend(needs(jobs.get(job, {})))
    return found


def steps(job: dict) -> list[dict]:
    return job.get("steps", [])


def build_steps(job: dict) -> list[dict]:
    return [s for s in steps(job) if str(s.get("uses", "")).startswith(BUILD_ACTION)]


def runs(job: dict) -> list[str]:
    """Each step's run script, or "" for a step that uses an action, in step order."""
    return [str(s.get("run", "")) for s in steps(job)]


def step_index(job: dict, command: str) -> int | None:
    return next((i for i, run in enumerate(runs(job)) if command in run), None)


def needs(job: dict) -> list[str]:
    given = job.get("needs", [])
    return [given] if isinstance(given, str) else list(given)


def platforms_of(job: dict, value: object) -> set[str]:
    """The platforms a build step names, resolving `${{ matrix.<key> }}` against the job's matrix."""
    values = value if isinstance(value, list) else str(value).split(",")
    found: set[str] = set()
    for v in (str(v).strip() for v in values):
        ref = MATRIX_REF.match(v)
        if not ref:
            found.add(v)
            continue
        matrix = job.get("strategy", {}).get("matrix", {})
        listed = matrix.get(ref.group(1), [])
        found |= {str(p) for p in (listed if isinstance(listed, list) else [listed])}
        found |= {str(row[ref.group(1)]) for row in matrix.get("include", []) if ref.group(1) in row}
    return {p for p in found if p}


def report(errors: list[str]) -> int:
    for e in errors:
        print(f"release check: {e}", file=sys.stderr)
    if not errors:
        print(
            "release check: release.yml builds, merges and signs one index for linux/amd64 and linux/arm64,"
            " attaches the signed chart, builds, sums and signs the CLI, server and oml-web archives of every"
            " target, which ci.yml's cli-matrix builds and tests first, and sums and signs the Claude Code"
            " plugin archive, the pinned marketplace, the agent kit and the SDK packages, which only two"
            " jobs gated on PUBLISH_SDKS publish; the installers fetch only the"
            " signed CLI archive, SHA256SUMS and its bundle, and ci.yml's install-smoke runs them"
        )
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
