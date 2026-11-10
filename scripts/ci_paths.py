#!/usr/bin/env python3
"""Which CI jobs a change must run: the paths a pull request changed, sorted into categories.

    python scripts/ci_paths.py --event pull_request --base-ref main --base SHA --head SHA >> "$GITHUB_OUTPUT"
    python scripts/ci_paths.py --files docs/index.md crates/oml-scan/src/lib.rs   # a list instead of git

Prints one `<category>=true|false` line per category. The workflows' `changes` jobs call it and
gate each job on `needs.changes.outputs.<category> == 'true'`; scripts/tests/test_ci_paths.py feeds
it sample file lists and checks the workflows against it. The table in
docs/contributing/ci-and-toolchain.md ("Which jobs a change runs") is this file's CATEGORIES.

Everything runs, with no filtering, when the event is not a pull_request into main (a push to
main, a release branch, a tag, a merge group), when the diff cannot be computed, and when any
changed path matches no category. A change to .github/, Cargo.lock, rust-toolchain.toml or
scripts/ is category `ci`, which turns every category on. Standard library only.
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys

# Category -> path globs. `**` crosses `/`, `*` does not. A path may be in several categories.
CATEGORIES: dict[str, list[str]] = {
    # Compiled into or read by the workspace's own tests.
    "rust": ["crates/**", "Cargo.toml", "deny.toml", "testing/lake/**", "testing/oml-port-contracts/**", "testing/oml-test-pg/**"],
    "migrations": ["migrations/**"],  # sqlx::migrate! embeds them; the Postgres suites apply them
    # The machine-readable contracts the server include_str!s and its tests read.
    "api": ["docs/api/*.yaml", "docs/api/*.json"],
    # Pages and what the site build reads. Rust tests read some pages (see DOCS_READ_BY_RUST).
    "docs": ["docs/**", "design/**", "mkdocs.yml", "requirements.txt", "changes/**", "*.md", "LICENSE", "LICENSE-docs", "LICENSES/**", "hooks/**", "site/**"],  # hooks/ holds the mkdocs build hook; site/ the landing
    "cli_ref": ["docs/cli/**"],  # compared with the built oml binary's help
    "plugins": ["plugins/**", "kit/**", ".claude-plugin/**"],
    "sdk": ["sdks/**"],
    "eval": ["eval/**", "bench/**"],
    "release": ["Dockerfile", ".dockerignore", "docker-compose.yml", "deploy/**", "LICENSES/**"],  # the image copies LICENSES/
    "spikes": ["testing/spikes/**"],
    # Runs everything: the workflows, the lockfile, the toolchain pin and the scripts the jobs run.
    "ci": [".github/**", "Cargo.lock", "rust-toolchain.toml", "scripts/**"],
}

# Derived outputs, so a workflow condition names one thing. Each is an OR of categories.
COMPOSITES: dict[str, list[str]] = {
    # Anything the Rust build or the full workspace suite takes as input.
    "cargo": ["rust", "api", "migrations"],
    # Files outside the build that Rust tests read: docs/** (oml-scan's corpus, connect-page.md,
    # roles-and-keys.md, cli/reference.md, adr/) and eval/** (the recall corpus, the near-copy data,
    # the bench results). A change here runs just those test targets, not the workspace.
    "docs_tests": ["docs", "eval"],
}

OUTPUTS = [*CATEGORIES, *COMPOSITES]


def _regex(glob: str) -> re.Pattern[str]:
    out, i = "", 0
    while i < len(glob):
        if glob.startswith("**", i):
            out += ".*"
            i += 2
        elif glob[i] == "*":
            out += "[^/]*"
            i += 1
        else:
            out += re.escape(glob[i])
            i += 1
    return re.compile(out + r"\Z")


_PATTERNS = {c: [_regex(g) for g in gs] for c, gs in CATEGORIES.items()}


def categorize(files: list[str]) -> dict[str, bool]:
    """Every output name -> whether a job gated on it runs for these changed files."""
    on = {c: False for c in CATEGORIES}
    for f in files:
        hit = [c for c, ps in _PATTERNS.items() if any(p.match(f) for p in ps)]
        if not hit:
            print(f"ci_paths: {f} matches no category, so everything runs", file=sys.stderr)
            return everything()
        for c in hit:
            on[c] = True
    if on["ci"]:
        return everything()
    for name, parts in COMPOSITES.items():
        on[name] = any(on[p] for p in parts)
    return on


def everything() -> dict[str, bool]:
    return {name: True for name in OUTPUTS}


def changed_files(base: str, head: str, root: str = ".") -> list[str] | None:
    """Paths changed on the head since it forked from the base; renames count both sides."""
    r = subprocess.run(["git", "diff", "--name-only", "--no-renames", f"{base}...{head}"],
                       cwd=root, capture_output=True, text=True)
    if r.returncode != 0:
        print(f"ci_paths: git diff failed: {r.stderr.strip()}", file=sys.stderr)
        return None
    return [l for l in r.stdout.splitlines() if l]


def select(event: str, base_ref: str, files: list[str] | None) -> dict[str, bool]:
    if event != "pull_request" or base_ref != "main" or files is None:
        return everything()
    return categorize(files)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--event", default="pull_request")
    ap.add_argument("--base-ref", default="main", help="the branch the pull request targets")
    ap.add_argument("--base", help="base commit")
    ap.add_argument("--head", help="head commit")
    ap.add_argument("--files", nargs="*", help="changed paths, instead of a git diff")
    ap.add_argument("--root", default=".")
    a = ap.parse_args(argv)
    files = a.files
    if files is None and a.event == "pull_request":
        files = changed_files(a.base, a.head, a.root) if a.base and a.head else None
    on = select(a.event, a.base_ref, files)
    for name in OUTPUTS:
        print(f"{name}={'true' if on[name] else 'false'}")
    print("ci_paths: " + (", ".join(n for n in OUTPUTS if on[n]) or "nothing") + f" ({len(files or [])} files)",
          file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
