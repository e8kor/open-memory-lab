"""Package the agent artifacts a release ships beside the binaries.

Run by .github/workflows/release.yml (job agents) and, into a temporary
directory, by scripts/check_agent_kit.py. Writes into --out:

- oml-claude-plugins-X.Y.Z.tar.gz and .zip: one directory holding
  .claude-plugin/marketplace.json, plugins/oml, plugins/oml-admin and LICENSE,
  copied from the repository's tracked files. The marketplace's sources are
  the repository's own relative paths, so they resolve inside the archive and
  `/plugin marketplace add <extracted directory>` installs offline;
- oml-marketplace-X.Y.Z.json: the same marketplace with every source a
  `git-subdir` of this repository at the tag vX.Y.Z (and the tag's commit when
  --sha is given), so adding it installs exactly that release;
- oml-agent-kit-X.Y.Z.tar.gz and .zip: kit/README.md, an AGENTS.md block whose
  sections are copied from the oml plugin's skill, and MCP configurations for
  OpenAI Codex CLI, Cursor, Gemini CLI, VS Code and Claude Desktop, all
  generated here from the constants below. Gemini CLI also gets the plugin's
  SessionStart command, read from plugins/oml/hooks/hooks.json.

The citations for every configuration format are in kit/README.md, with the
dates they were fetched. Archives are deterministic: sorted entries, fixed
timestamps and modes.

Dependencies: stdlib.
"""

import argparse
import gzip
import io
import json
import re
import subprocess
import sys
import tarfile
import tomllib
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKILL = ROOT / "plugins/oml/skills/oml-memory/SKILL.md"
HOOKS = ROOT / "plugins/oml/hooks/hooks.json"
KIT_README = ROOT / "kit/README.md"
MARKETPLACE = ROOT / ".claude-plugin/marketplace.json"
DEFAULT_REPO = "e8kor/open-memory-lab"

# Every configuration names the default server's endpoint literally and reads
# the key from OML_API_KEY (ADR-0043 d6, amending ADR-0039 d15): DEFAULT_URL,
# the maintainer's public server and the default of every integration a
# release ships, which check_agent_kit.py holds equal to DEFAULT_URL in
# crates/bins/oml-cli/src/setup.rs (ADR-0043 d1). None reads OML_URL, which
# an unset variable would leave as /mcp; a self-hoster edits the one URL.
KEY_VAR = "OML_API_KEY"
DEFAULT_URL = "https://memory.doublegate.io"
ENDPOINT = f"{DEFAULT_URL}/mcp"
# The stdio bridge Claude Desktop's config file needs for a remote server,
# pinned to a release older than two weeks on 2026-11-03.
MCP_REMOTE = "mcp-remote@0.15.2"

# The skill's sections the AGENTS.md block carries, by heading. "One
# connection, one key" is left out: it describes the Claude Code plugin's
# connection, which the kit's tools do not have.
AGENTS_SECTIONS = (
    None,  # the title and the paragraph before the first section
    "Three tools, chosen by their arguments",
    "Name a workarea",
    "Check what a workarea allows",
    "Recall before you act",
    "See what became of your writes",
    "Read one memory back",
    "Remember one claim at a time",
    "Revise or retire a memory",
    "What the tools cannot do",
)
AGENTS_BEGIN = "<!-- oml:begin (generated from plugins/oml/skills/oml-memory/SKILL.md) -->"
AGENTS_END = "<!-- oml:end -->"

# A fixed timestamp for every archive entry: 1980-01-01, the earliest a zip
# entry can carry.
EPOCH = 315532800
ZIP_TIME = (1980, 1, 1, 0, 0, 0)


def workspace_version() -> str:
    return tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]


def skill_sections() -> dict:
    """The skill's body split at each `## ` heading; key None is the part
    before the first one. Each value is the section's text, heading included,
    without trailing blank lines."""
    text = SKILL.read_text()
    body = re.sub(r"\A---\n.*?\n---\n", "", text, count=1, flags=re.S).lstrip("\n")
    sections: dict = {}
    current = None
    lines: list[str] = []
    for line in body.splitlines():
        if line.startswith("## "):
            sections[current] = "\n".join(lines).rstrip("\n")
            current, lines = line[3:].strip(), []
        lines.append(line)
    sections[current] = "\n".join(lines).rstrip("\n")
    return sections


def agents_block() -> str:
    sections = skill_sections()
    missing = [s for s in AGENTS_SECTIONS if s not in sections]
    if missing:
        raise SystemExit(f"package: {SKILL.relative_to(ROOT)} has no section {missing}")
    parts = [sections[s] for s in AGENTS_SECTIONS]
    return AGENTS_BEGIN + "\n\n" + "\n\n".join(parts) + "\n\n" + AGENTS_END + "\n"


def session_start_command() -> str:
    """The oml plugin's one SessionStart command, which check_plugin.py holds
    to ADR-0029 d7's guard."""
    hooks = json.loads(HOOKS.read_text())["hooks"]["SessionStart"]
    return hooks[0]["hooks"][0]["command"]


def kit_configs() -> dict:
    """Each kit path and its text."""
    bearer_env = f"Bearer ${{env:{KEY_VAR}}}"
    codex = (
        "# OpenAI Codex CLI: add to ~/.codex/config.toml, or to .codex/config.toml in a\n"
        "# trusted project. Codex sends the value of OML_API_KEY as the bearer token.\n"
        f"# The URL is the maintainer's public server; for your own, replace\n"
        f"# {DEFAULT_URL} with its address, keeping /mcp.\n"
        "[mcp_servers.oml]\n"
        f'url = "{ENDPOINT}"\n'
        f'bearer_token_env_var = "{KEY_VAR}"\n'
    )
    cursor = {"mcpServers": {"oml": {"url": ENDPOINT, "headers": {"Authorization": bearer_env}}}}
    gemini = {
        "mcpServers": {
            "oml": {"httpUrl": ENDPOINT, "headers": {"Authorization": f"Bearer ${{{KEY_VAR}}}"}}
        },
        "hooks": {
            "SessionStart": [
                {
                    "hooks": [
                        {
                            "name": "oml-session-start",
                            "type": "command",
                            "command": session_start_command(),
                            "timeout": 10000,
                        }
                    ]
                }
            ]
        },
    }
    vscode = {"servers": {"oml": {"type": "http", "url": ENDPOINT, "headers": {"Authorization": bearer_env}}}}
    desktop = {
        "mcpServers": {
            "oml": {
                "command": "npx",
                "args": [
                    "-y",
                    MCP_REMOTE,
                    ENDPOINT,
                    "--transport",
                    "http-only",
                    "--header",
                    f"Authorization: Bearer ${{{KEY_VAR}}}",
                ],
            }
        }
    }

    def js(value: dict) -> str:
        return json.dumps(value, indent=2) + "\n"

    return {
        "codex/config.toml": codex,
        "cursor/.cursor/mcp.json": js(cursor),
        "gemini/.gemini/settings.json": js(gemini),
        "vscode/.vscode/mcp.json": js(vscode),
        "claude-desktop/claude_desktop_config.json": js(desktop),
    }


def kit_files(version: str) -> dict:
    files = {"README.md": KIT_README.read_text().replace("@VERSION@", version), "AGENTS.md": agents_block()}
    files.update(kit_configs())
    return {path: (text.encode(), 0o644) for path, text in files.items()}


def tracked(paths: list[str]) -> list[str]:
    out = subprocess.run(
        ["git", "-C", str(ROOT), "ls-files", "-z", "--", *paths], capture_output=True, check=True
    ).stdout
    return sorted(p for p in out.decode().split("\0") if p)


def plugin_files() -> dict:
    files = {}
    for path in tracked([".claude-plugin/marketplace.json", "plugins/oml", "plugins/oml-admin", "LICENSE"]):
        source = ROOT / path
        files[path] = (source.read_bytes(), 0o755 if source.stat().st_mode & 0o111 else 0o644)
    return files


def pinned_marketplace(version: str, repo: str, sha: str | None) -> str:
    """The repository's marketplace with every relative source replaced by a
    git-subdir of `repo` at the tag v`version`."""
    marketplace = json.loads(MARKETPLACE.read_text())
    for entry in marketplace["plugins"]:
        source = entry["source"]
        if not (isinstance(source, str) and source.startswith("./")):
            raise SystemExit(f"package: marketplace entry {entry['name']} has no relative source")
        pinned = {
            "source": "git-subdir",
            "url": f"https://github.com/{repo}.git",
            "path": source[2:],
            "ref": f"v{version}",
        }
        if sha:
            pinned["sha"] = sha
        entry["source"] = pinned
    return json.dumps(marketplace, indent=2) + "\n"


def write_archives(out: Path, stem: str, files: dict) -> list[Path]:
    """`stem`.tar.gz and `stem`.zip, each holding `files` under the directory `stem`/."""
    names = sorted(files)
    dirs = sorted({f"{stem}/" + "/".join(p.split("/")[:i]) for p in names for i in range(1, p.count("/") + 1)} | {stem})

    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.PAX_FORMAT) as tar:
        for d in dirs:
            info = tarfile.TarInfo(d)
            info.type, info.mode, info.mtime = tarfile.DIRTYPE, 0o755, EPOCH
            tar.addfile(info)
        for name in names:
            data, mode = files[name]
            info = tarfile.TarInfo(f"{stem}/{name}")
            info.size, info.mode, info.mtime = len(data), mode, EPOCH
            tar.addfile(info, io.BytesIO(data))
    tgz = out / f"{stem}.tar.gz"
    with open(tgz, "wb") as f, gzip.GzipFile(filename="", mode="wb", fileobj=f, mtime=0) as gz:
        gz.write(raw.getvalue())

    zpath = out / f"{stem}.zip"
    with zipfile.ZipFile(zpath, "w", zipfile.ZIP_DEFLATED) as z:
        for d in dirs:
            info = zipfile.ZipInfo(d + "/", ZIP_TIME)
            info.external_attr = (0o40755 << 16) | 0x10
            z.writestr(info, b"")
        for name in names:
            data, mode = files[name]
            info = zipfile.ZipInfo(f"{stem}/{name}", ZIP_TIME)
            info.external_attr = (0o100000 | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data)
    return [tgz, zpath]


def package(out: Path, version: str, repo: str, sha: str | None) -> list[Path]:
    out.mkdir(parents=True, exist_ok=True)
    written = write_archives(out, f"oml-claude-plugins-{version}", plugin_files())
    pinned = out / f"oml-marketplace-{version}.json"
    pinned.write_text(pinned_marketplace(version, repo, sha))
    written.append(pinned)
    written += write_archives(out, f"oml-agent-kit-{version}", kit_files(version))
    return written


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--version", help="must equal the workspace version; defaults to it")
    parser.add_argument("--repo", default=DEFAULT_REPO, help="owner/repo the pinned marketplace fetches from")
    parser.add_argument("--sha", help="the tag's commit, pinned beside the tag in the pinned marketplace")
    args = parser.parse_args()
    version = workspace_version()
    if args.version and args.version != version:
        print(f"package: --version {args.version} is not the workspace version {version}", file=sys.stderr)
        return 1
    if args.sha and not re.fullmatch(r"[0-9a-f]{40}", args.sha):
        print(f"package: --sha {args.sha} is not a full lowercase commit SHA", file=sys.stderr)
        return 1
    for path in package(args.out, version, args.repo, args.sha):
        print(path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
