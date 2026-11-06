#!/usr/bin/env python3
"""Write docs/operations/configuration.md from the server's setting keys and scripts/settings_notes.yaml.

The key names are the KEY_* constants in crates/bins/oml-server-lead/src/lib.rs, so a key the code
reads always appears and a key the code dropped cannot stay. The notes file carries the words and
the defaults. The script fails when the two disagree, and when a listed environment variable is
not read anywhere under crates/.

    python scripts/gen_settings_reference.py [--check]
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

import yaml

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import docgen  # noqa: E402

LIB = docgen.ROOT / "crates" / "bins" / "oml-server-lead" / "src" / "lib.rs"
NOTES = docgen.ROOT / "scripts" / "settings_notes.yaml"
OUT = docgen.DOCS / "operations" / "configuration.md"


def env_name(key: str) -> str:
    return "OML_" + key.upper().replace(".", "__")


def code_keys() -> list[str]:
    return re.findall(r'^const KEY_[A-Z0-9_]+: &str = "([a-z0-9_.]+)";', LIB.read_text(), re.M)


def env_read_in_code(name: str) -> bool:
    for path in (docgen.ROOT / "crates").rglob("*.rs"):
        if "fixtures" in path.parts or "tests" in path.parts:
            continue
        if f'"{name}"' in path.read_text(errors="ignore"):
            return True
    return False


def render(notes: dict) -> tuple[str, list[str]]:
    problems = []
    keys = code_keys()
    server = notes["server"]
    for k in keys:
        if k not in server:
            problems.append(f"the code reads `{k}`; add it to scripts/settings_notes.yaml")
    for k in server:
        if k not in keys:
            problems.append(f"scripts/settings_notes.yaml names `{k}`, which the code no longer reads")
    for section in ("lakewriter", "client"):
        for name in notes[section]:
            if not env_read_in_code(name):
                problems.append(f"{name} is listed but no crate reads it")
    out = [
        docgen.front_matter("every server setting and environment variable the build reads, with its default"),
        "# Settings\n",
        "A server reads one TOML file, `oml.toml` by default (`--config <path>`), and the environment. "
        "The environment wins over the file, and the file wins over the defaults. "
        "An environment name is `OML_`, then the dotted key in capitals with `__` between the parts: "
        "`search.rerank.depth` becomes `OML_SEARCH__RERANK__DEPTH`.\n",
        f"*{docgen.BANNER.format(script='scripts/gen_settings_reference.py')} Keys come from `crates/bins/oml-server-lead/src/lib.rs`; words from `scripts/settings_notes.yaml`.*\n",
        "A value that does not parse stops the server at start and names the key. Per-tenant policy, such as a workarea's review rules, is not a setting: an administrator changes it with the [CLI](../cli/reference.md).\n",
        "## Server settings\n",
        "| Key | Environment variable | Default | What it does |\n|---|---|---|---|",
    ]
    for k in keys:
        n = server.get(k, {"default": "", "note": ""})
        out.append(f"| `{k}` | `{env_name(k)}` | {n['default']} | {n['note']} |")
    out.append("\n## Lake writer\n")
    out.append("The `oml-lakewriter` binary takes these as flags or environment variables. See [run the lake writer](lake.md).\n")
    out.append("| Variable | Default | What it does |\n|---|---|---|")
    for name, n in notes["lakewriter"].items():
        out.append(f"| `{name}` | {n.get('default', 'none')} | {n['note']} |")
    out.append("\n## Client and plugin\n")
    out.append("The CLI, the SDKs, the hooks and the agent kit read these. The Claude Code plugin reads its `url` and `api_key` options instead.\n")
    out.append("| Variable | What it does |\n|---|---|")
    for name, n in notes["client"].items():
        out.append(f"| `{name}` | {n['note']} |")
    return "\n".join(out), problems


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="fail if the page differs from the code and the notes")
    args = ap.parse_args()
    text, problems = render(yaml.safe_load(NOTES.read_text()))
    for p in problems:
        print(p, file=sys.stderr)
    if problems:
        return 1
    return docgen.finish(OUT, text, args.check)


if __name__ == "__main__":
    sys.exit(main())
