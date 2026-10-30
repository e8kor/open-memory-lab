"""The release workflow ships one signed image index for both platforms.

Run by .github/workflows/ci.yml (job image). Holds .github/workflows/release.yml
to docs/engineering/ci-and-toolchain.md "Release artifacts", failing with every
disagreement found:

- the image is built for linux/amd64 and linux/arm64, each build pushed by
  digest rather than by tag;
- a merge job, needing the build job, assembles the index with
  `docker buildx imagetools create`;
- that merge job signs the index digest with `cosign sign --recursive`, after it
  has asserted both platforms with `docker buildx imagetools inspect --raw`, and
  no build job signs a single-platform digest.

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

    return report(errors)


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
        print("release check: release.yml builds, merges and signs one index for linux/amd64 and linux/arm64")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
