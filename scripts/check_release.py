"""The release workflow ships one signed image index for both platforms, and the signed chart.

Run by .github/workflows/ci.yml (job image). Holds .github/workflows/release.yml
to docs/engineering/ci-and-toolchain.md "Release artifacts", failing with every
disagreement found:

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
  `gh release upload` (docs/adr/ADR-0033-interim-helm-chart.md d10).

Dependencies: stdlib + PyYAML.
"""

import re
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github/workflows/release.yml"
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

    check_chart(errors, jobs)
    return report(errors)


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
            " and attaches the signed chart"
        )
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
