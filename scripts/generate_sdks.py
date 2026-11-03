#!/usr/bin/env python3
"""Regenerate the SDKs' generated layers from docs/api/openapi.yaml.

The regeneration rule of docs/engineering/sdks.md: a change to openapi.yaml
regenerates both clients in the same change, and CI (job sdk-generated) runs
this script with --check, which fails when the committed code is stale.

  python3 scripts/generate_sdks.py           regenerate both layers in place
  python3 scripts/generate_sdks.py --check   regenerate into a temporary
                                             directory and fail on any
                                             difference

Each generator runs at the version its package manager pins:

  Python      openapi-python-client from the `generate` dependency group of
              sdks/python/pyproject.toml, locked in sdks/python/uv.lock, run
              with `uv run --locked`; its warnings are compared with the
              recorded report in sdks/python/generator-warnings.txt, so a new
              gap in the generated layer fails here instead of landing unseen.
  TypeScript  openapi-typescript from sdks/typescript/package.json, locked in
              sdks/typescript/package-lock.json, run with
              --default-non-nullable false; `npm ci` in sdks/typescript must
              have run first.

Dependencies: Python 3.11+ standard library, uv and an installed
sdks/typescript/node_modules. Exit status 0 when current (or regenerated),
1 with the differing paths otherwise.
"""

from __future__ import annotations

import argparse
import filecmp
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPEC = ROOT / "docs" / "api" / "openapi.yaml"
PYTHON = ROOT / "sdks" / "python"
PYTHON_OUT = PYTHON / "src" / "oml_sdk" / "generated"
PYTHON_WARNINGS = PYTHON / "generator-warnings.txt"
TYPESCRIPT = ROOT / "sdks" / "typescript"
TYPESCRIPT_OUT = TYPESCRIPT / "src" / "generated" / "schema.ts"
TYPESCRIPT_GENERATOR = TYPESCRIPT / "node_modules" / ".bin" / "openapi-typescript"

# The generator prints the address of an object in one of its messages.
ADDRESS_RE = re.compile(r" object at 0x[0-9a-f]+")


def fail(message: str) -> None:
    sys.exit(f"generate_sdks: {message}")


def normalized_warnings(stderr: str) -> str:
    lines = [ADDRESS_RE.sub(" object", line).rstrip() for line in stderr.splitlines()]
    return "\n".join(line for line in lines if line) + "\n"


def generate_python(out: Path) -> str:
    """Generate the Python layer into `out`; return its normalized warnings."""
    done = subprocess.run(
        [
            "uv", "run", "--quiet", "--project", str(PYTHON), "--locked", "--only-group", "generate",
            "openapi-python-client", "generate",
            "--path", str(SPEC),
            "--meta", "none",
            "--config", str(PYTHON / "openapi-python-client.yaml"),
            "--output-path", str(out),
        ],
        capture_output=True,
        text=True,
    )  # fmt: skip
    if done.returncode != 0 or not (out / "__init__.py").is_file():
        fail(f"openapi-python-client failed:\n{done.stdout}\n{done.stderr}")
    return normalized_warnings(done.stderr)


def generate_typescript(out: Path) -> None:
    if not TYPESCRIPT_GENERATOR.is_file():
        fail("openapi-typescript is not installed: run `npm ci` in sdks/typescript first")
    done = subprocess.run(
        # A field with a schema default stays optional: a request body sends
        # only what the caller set, and the server applies the default.
        [str(TYPESCRIPT_GENERATOR), str(SPEC), "--default-non-nullable", "false", "--output", str(out)],
        cwd=TYPESCRIPT,
        capture_output=True,
        text=True,
    )
    if done.returncode != 0 or not out.is_file():
        fail(f"openapi-typescript failed:\n{done.stdout}\n{done.stderr}")


def tree_differences(want: Path, have: Path) -> list[str]:
    """Every file path under either tree that is missing from one or differs."""
    differences = []
    want_files = {p.relative_to(want) for p in want.rglob("*") if p.is_file()}
    have_files = (
        {p.relative_to(have) for p in have.rglob("*") if p.is_file() and "__pycache__" not in p.parts}
        if have.is_dir()
        else set()
    )
    for path in sorted(want_files | have_files):
        if path not in have_files:
            differences.append(f"missing: {have / path}")
        elif path not in want_files:
            differences.append(f"not generated: {have / path}")
        elif not filecmp.cmp(want / path, have / path, shallow=False):
            differences.append(f"stale: {have / path}")
    return differences


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="fail if the generated code is stale")
    args = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="oml-sdks-") as tmp:
        python_out = Path(tmp) / "generated"
        typescript_out = Path(tmp) / "schema.ts"
        warnings = generate_python(python_out)
        generate_typescript(typescript_out)

        if not args.check:
            shutil.rmtree(PYTHON_OUT, ignore_errors=True)
            shutil.copytree(python_out, PYTHON_OUT)
            TYPESCRIPT_OUT.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(typescript_out, TYPESCRIPT_OUT)
            PYTHON_WARNINGS.write_text(warnings)
            print("generate_sdks: regenerated both SDKs from docs/api/openapi.yaml")
            return 0

        problems = tree_differences(python_out, PYTHON_OUT)
        if not TYPESCRIPT_OUT.is_file():
            problems.append(f"missing: {TYPESCRIPT_OUT}")
        elif not filecmp.cmp(typescript_out, TYPESCRIPT_OUT, shallow=False):
            problems.append(f"stale: {TYPESCRIPT_OUT}")
        recorded = PYTHON_WARNINGS.read_text() if PYTHON_WARNINGS.is_file() else None
        if recorded != warnings:
            problems.append(
                f"the generator's warnings differ from {PYTHON_WARNINGS}:\n{warnings}"
            )

    if problems:
        print("generate_sdks: the generated SDK code is not current with docs/api/openapi.yaml")
        for problem in problems:
            print(f"  {problem}")
        print("run `python3 scripts/generate_sdks.py` and commit the result")
        return 1
    print("generate_sdks: both SDKs are current with docs/api/openapi.yaml")
    return 0


if __name__ == "__main__":
    sys.exit(main())
