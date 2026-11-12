#!/usr/bin/env python3
"""No surface this repository releases names a server it was not given
(ADR-0076 d1, d10 test 1).

Scans every tracked text file, docs and README.md included, so a page cannot
bring a default back, and fails on:

- the hosted service's names: the pattern `doublegate\\.io`, and the phrases
  "maintainer's public server" and "the public server", in any case;
- a constant named DEFAULT_URL, DEFAULT_SERVER or PUBLIC_SERVER;
- a `default` on the oml plugin's `url` option.

The pattern is narrow on purpose: an allowlist of hosts over crates/ would
trip on scan-rule fixtures and cited references. Left out are the records
and history that name the hosted service as a fact of the past: the ADRs,
the changelog and its fragments, the number registry and the generated
design-records page that list ADR titles, the hosted service's legal drafts
and landing (which leave with #194), the landing's test, and the frozen ADR
copies the sync tests read as data.

Run by .github/workflows/practices.yml. Dependencies: stdlib and git.
Exit status 0 when nothing is found, 1 with each finding otherwise.
"""
from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXCLUDED = (
    "docs/adr/",
    "docs/changelog.md",
    "changes/",
    "docs/NUMBERS.md",
    "docs/reference/design-records.md",
    "docs/legal/",
    "site/",
    "scripts/tests/test_site_build.py",
    "crates/bins/oml-cli/tests/fixtures/",
    "scripts/check_no_default_server.py",
)
HOSTED = re.compile(r"doublegate\.io|maintainer'?s public server|\bthe public server\b", re.I)
CONSTANT = re.compile(r"\b(DEFAULT_URL|DEFAULT_SERVER|PUBLIC_SERVER)\b")
PLUGIN = "plugins/oml/.claude-plugin/plugin.json"


def tracked() -> list[str]:
    out = subprocess.run(
        ["git", "-C", str(ROOT), "ls-files", "-z"], check=True, capture_output=True
    ).stdout
    return [p for p in out.decode().split("\0") if p and not p.startswith(EXCLUDED)]


def findings() -> list[str]:
    found: list[str] = []
    for path in tracked():
        try:
            # errors="replace": a file that is not valid UTF-8 is still read, so
            # a name cannot hide in one.
            text = (ROOT / path).read_text(encoding="utf-8", errors="replace")
        except (FileNotFoundError, IsADirectoryError):
            continue
        for number, line in enumerate(text.splitlines(), 1):
            for pattern, what in ((HOSTED, "names the hosted service"), (CONSTANT, "declares a default-server constant")):
                match = pattern.search(line)
                if match:
                    found.append(f"{path}:{number}: {what} ({match.group(0)!r})")
    url = json.loads((ROOT / PLUGIN).read_text()).get("userConfig", {}).get("url", {})
    if "default" in url:
        found.append(f"{PLUGIN}: the url option declares a default ({url['default']!r})")
    return found


def main() -> int:
    found = findings()
    for line in found:
        print(f"no default server: {line}", file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
