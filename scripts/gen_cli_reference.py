#!/usr/bin/env python3
"""Write docs/cli/reference.md from the `oml` binary's own help.

It walks `oml --help` and every subcommand's `--help` and renders one page, so the reference
lists exactly the verbs and flags the build has. Build the binary first (`cargo build -p oml-cli`).

    python scripts/gen_cli_reference.py [--oml PATH] [--check]

The binary runs with an empty environment, so the page does not depend on the machine's OML_* values.
"""

from __future__ import annotations

import argparse
import os
import pathlib
import re
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import docgen  # noqa: E402

OUT = docgen.DOCS / "cli" / "reference.md"
GLOBAL = {"--url", "--api-key", "--json", "--help", "--version"}


def find_binary(explicit: str | None) -> str:
    for cand in (explicit, os.environ.get("OML_BIN"), docgen.ROOT / ".tmp-target/debug/oml", docgen.ROOT / "target/debug/oml"):
        if cand and pathlib.Path(cand).is_file():
            return str(cand)
    sys.exit("no oml binary: run `cargo build -p oml-cli` or pass --oml PATH")


def help_of(oml: str, path: list[str]) -> str:
    env = {"HOME": "/nonexistent", "PATH": "/usr/bin:/bin", "COLUMNS": "10000"}
    out = subprocess.run([oml, *path, "--help"], env=env, capture_output=True, text=True, check=True)
    if re.search(r"\bADR-\d", out.stdout):
        command = " ".join(["oml", *path])
        sys.exit(f"{command} --help names a design record; keep CLI help self-contained")
    return out.stdout


def sections(text: str) -> dict[str, list[str]]:
    """Split clap help into its head (text before Usage) and the Arguments/Commands/Options blocks."""
    out: dict[str, list[str]] = {"head": []}
    cur = "head"
    for line in text.splitlines():
        m = re.match(r"^(Usage|Arguments|Commands|Options):\s*(.*)$", line)
        if m:
            cur = m.group(1)
            out[cur] = [m.group(2)] if m.group(2) else []
        else:
            out[cur].append(line)
    return out


def entries(block: list[str]) -> list[tuple[str, str]]:
    """clap prints `  name  description`, or the description on the lines below a long name."""
    res: list[tuple[str, str]] = []
    for line in block:
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip())
        if indent <= 6 and (line.lstrip().startswith(("-", "<", "[")) or res == [] or indent <= 2):
            parts = re.split(r"\s{2,}", line.strip(), maxsplit=1)
            res.append((parts[0], parts[1] if len(parts) > 1 else ""))
        elif res:
            name, desc = res[-1]
            res[-1] = (name, (desc + " " + line.strip()).strip())
    return res


def clean(text: str) -> str:
    text = re.sub(r"\s*\[env: [^\]]*\]", "", text)
    return docgen.strip_handles(text)


def first_sentence(text: str) -> str:
    m = re.match(r"(.+?[.!?])(\s|$)", text)
    return m.group(1) if m else text


def walk(oml: str, path: list[str], acc: list[dict]) -> None:
    sec = sections(help_of(oml, path))
    node = {
        "path": path,
        "about": clean(" ".join(l.strip() for l in sec["head"] if l.strip())),
        "usage": " ".join(sec.get("Usage", [""])).strip(),
        "args": [(n, clean(d)) for n, d in entries(sec.get("Arguments", []))],
        "opts": [(n, clean(d)) for n, d in entries(sec.get("Options", []))],
        "subs": [(n, clean(d)) for n, d in entries(sec.get("Commands", [])) if n != "help"],
    }
    acc.append(node)
    for name, _ in node["subs"]:
        walk(oml, path + [name], acc)


def md_cell(text: str) -> str:
    return text.replace("|", "\\|")


def render(nodes: list[dict]) -> str:
    leaves = [n for n in nodes if n["path"] and not n["subs"]]
    top = [n for n in nodes if len(n["path"]) == 1]
    root = nodes[0]
    out = [docgen.front_matter("every oml command, subcommand, argument and option, as the build prints them"), "# CLI reference\n"]
    out.append(f"`oml` is the command-line client for administrators, reviewers and scripts. It calls the same HTTP API as agents and holds no authority of its own.\n")
    out.append(f"*{docgen.BANNER.format(script='scripts/gen_cli_reference.py')} It lists {len(leaves)} commands.*\n")
    out.append("## Options every command takes\n")
    out.append("| Option | Meaning |\n|---|---|")
    for n, d in root["opts"]:
        key = n.split()[0].lstrip("-,") if n.startswith("-h") else n
        out.append(f"| `{n}` | {md_cell(d)} |")
    out.append("\nPer-command sections below omit these options.\n")
    out.append("## All commands\n")
    out.append("| Command | What it does |\n|---|---|")
    for n in leaves:
        cmd = "oml " + " ".join(n["path"])
        out.append(f"| `{cmd}` | {md_cell(first_sentence(n['about']))} |")
    for t in top:
        out.append(f"\n## `oml {t['path'][0]}`\n")
        out.append(t["about"] + "\n")
        group = [n for n in nodes if n["path"][:1] == t["path"] and (n is t or True)]
        for n in group:
            if n["subs"] and n is not t:
                out.append(f"### `oml {' '.join(n['path'])}`\n\n{n['about']}\n")
                continue
            if n is t and n["subs"]:
                continue
            out.append(f"### `oml {' '.join(n['path'])}`\n" if n is not t else "")
            if n is not t:
                out.append(n["about"] + "\n")
            usage = re.sub(r"^oml ", "oml ", n["usage"])
            out.append(f"```\n{usage}\n```\n")
            if n["args"]:
                out.append("| Argument | Meaning |\n|---|---|")
                out += [f"| `{a}` | {md_cell(d)} |" for a, d in n["args"]]
                out.append("")
            opts = [(a, d) for a, d in n["opts"] if re.findall(r"--[a-z-]+", a)[:1] and re.findall(r"--[a-z-]+", a)[0] not in GLOBAL]
            if opts:
                out.append("| Option | Meaning |\n|---|---|")
                out += [f"| `{a}` | {md_cell(d)} |" for a, d in opts]
                out.append("")
    return "\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--oml", help="path to the oml binary")
    ap.add_argument("--check", action="store_true", help="fail if the page differs from the binary's help")
    args = ap.parse_args()
    oml = find_binary(args.oml)
    nodes: list[dict] = []
    walk(oml, [], nodes)
    return docgen.finish(OUT, docgen.mark_unreleased(render(nodes), "cli"), args.check)


if __name__ == "__main__":
    sys.exit(main())
