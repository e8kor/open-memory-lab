"""The agent plugin agrees with the server it connects to, its SessionStart hook
is guarded the way ADR-0029 d7 decides, and the admin plugin issues keys only
the way ADR-0028 d3 allows.

Run by .github/workflows/ci.yml (job plugin). Checks, failing on the first
class of disagreement found:

- the marketplace lists the plugin at a directory that has a manifest;
- the plugin's version is the workspace version (Cargo.toml), so a release tag
  names one version for the image, the binaries and the plugin;
- .mcp.json declares exactly one server, `oml`: one key does the agent's whole
  loop (docs/api/mcp-plugin.md, "Wiring it into your agent"; ADR-0025);
- every MCP entry targets the server's /mcp endpoint with a bearer key;
- the skill and the plugin README name exactly the tools the server serves
  (SERVED_TOOLS in crates/transports/oml-transport-http/src/mcp.rs), and every
  served tool is declared in docs/api/mcp-tools.json;
- plugins/oml/hooks/hooks.json exists and parses, and its SessionStart hooks
  are exactly one command, which, run under /bin/sh, exits 0 with no output
  when no `oml` is on PATH, and otherwise runs `oml` with exactly the
  arguments `hook session-start` and exits 0 even when that `oml` fails
  (docs/cli/reference.md, "oml hook session-start"; ADR-0029 d7);
- the marketplace lists oml-admin at a directory that has a manifest, at the
  workspace version;
- oml-admin has no .mcp.json, so it adds no tool to any tool list;
- every command in the oml-admin skill that runs `issue-agent` carries `--out`
  and never `--json`, so the key value never reaches the model's context
  (docs/cli/reference.md, "The oml-admin plugin").
"""

import json
import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ADMIN_SKILL = "skills/issue-agent-key/SKILL.md"
HOOKS = "hooks/hooks.json"
# A fenced block, indented or not, and an inline code span outside one.
FENCE = re.compile(r"^[ \t]*```[^\n]*\n(.*?)^[ \t]*```", re.S | re.M)
SPAN = re.compile(r"`([^`\n]+)`")


def main() -> int:
    errors: list[str] = []

    marketplace = json.loads((ROOT / ".claude-plugin/marketplace.json").read_text())
    entries = {p["name"]: p for p in marketplace["plugins"]}
    if "oml" not in entries:
        errors.append("marketplace.json does not list the oml plugin")
        return report(errors)
    plugin_dir = (ROOT / entries["oml"]["source"]).resolve()
    manifest_path = plugin_dir / ".claude-plugin/plugin.json"
    if not manifest_path.is_file():
        errors.append(f"{manifest_path} is missing")
        return report(errors)
    manifest = json.loads(manifest_path.read_text())

    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())
    workspace_version = cargo["workspace"]["package"]["version"]
    if manifest["version"] != workspace_version:
        errors.append(
            f"plugin.json version {manifest['version']} != workspace version {workspace_version}"
        )

    servers = json.loads((plugin_dir / ".mcp.json").read_text())["mcpServers"]
    if list(servers) != ["oml"]:
        errors.append(f".mcp.json declares {sorted(servers)}, not exactly the one server oml")
    for name, server in servers.items():
        if server.get("type") != "http" or not server.get("url", "").endswith("/mcp"):
            errors.append(f".mcp.json {name}: not an http entry ending in /mcp")
        if not server.get("headers", {}).get("Authorization", "").startswith("Bearer ${"):
            errors.append(f".mcp.json {name}: Authorization is not a bearer key from the environment")

    mcp_rs = (ROOT / "crates/transports/oml-transport-http/src/mcp.rs").read_text()
    match = re.search(r"SERVED_TOOLS: \[&str; \d+\] = \[([^\]]*)\]", mcp_rs)
    if not match:
        errors.append("SERVED_TOOLS not found in mcp.rs")
        return report(errors)
    served = set(re.findall(r'"(oml_[a-z_]+)"', match.group(1)))

    catalog = {t["name"] for t in json.loads((ROOT / "docs/api/mcp-tools.json").read_text())["tools"]}
    for tool in sorted(served - catalog):
        errors.append(f"{tool} is served but not declared in docs/api/mcp-tools.json")

    for doc in [plugin_dir / "skills/oml-memory/SKILL.md", plugin_dir / "README.md"]:
        text = doc.read_text()
        named = set(re.findall(r"`(oml_[a-z_]+)`", text))
        for tool in sorted(served - named):
            errors.append(f"{doc.relative_to(ROOT)} does not name the served tool {tool}")
        unbuilt_mentioned = named - served
        if doc.name == "SKILL.md" and unbuilt_mentioned:
            errors.append(
                f"{doc.relative_to(ROOT)} names tools the server does not serve: {sorted(unbuilt_mentioned)}"
            )

    check_hooks(plugin_dir, errors)
    check_admin(entries, workspace_version, errors)
    return report(errors)


def check_hooks(plugin_dir: Path, errors: list[str]) -> None:
    """The one SessionStart command runs `oml hook session-start` only when `oml`
    is on PATH, and a missing or failing `oml` exits 0, so a user without the
    CLI, or with a CLI older than the verb, sees no hook error (ADR-0029 d7)."""
    path = plugin_dir / HOOKS
    shown = path.relative_to(ROOT)
    if not path.is_file():
        errors.append(f"{shown} is missing: the oml plugin's SessionStart hook (ADR-0029 d7)")
        return
    try:
        hooks = json.loads(path.read_text())["hooks"]
        groups = hooks.get("SessionStart", [])
        entries = [h for group in groups for h in group["hooks"]]
    except (json.JSONDecodeError, KeyError, TypeError, AttributeError) as e:
        errors.append(f"{shown} does not parse as a hooks file ({type(e).__name__}: {e})")
        return
    if len(entries) != 1 or entries[0].get("type") != "command":
        types = [h.get("type") for h in entries]
        errors.append(f"{shown}: SessionStart holds the hooks {types}, not exactly one command")
        return
    command = entries[0].get("command", "")

    with tempfile.TemporaryDirectory() as tmp:
        no_oml = Path(tmp) / "no-oml"
        no_oml.mkdir()
        run = sh(command, no_oml)
        if (run.returncode, run.stdout, run.stderr) != (0, "", ""):
            errors.append(
                f"{shown}: with no oml on PATH the command exits {run.returncode} "
                f"(stdout {run.stdout!r}, stderr {run.stderr!r}), not 0 in silence: {command}"
            )

        # A stand-in `oml` that records its arguments and fails, as a CLI
        # older than the verb does.
        stand_in = Path(tmp) / "stand-in"
        stand_in.mkdir()
        argv = Path(tmp) / "argv"
        oml = stand_in / "oml"
        oml.write_text(f"#!/bin/sh\nprintf '%s\\n' \"$@\" > '{argv}'\nexit 2\n")
        oml.chmod(0o755)
        run = sh(command, stand_in)
        ran = argv.read_text().splitlines() if argv.exists() else None
        if ran != ["hook", "session-start"]:
            errors.append(f"{shown}: the command runs oml with {ran}, not `hook session-start`: {command}")
        if run.returncode != 0:
            errors.append(f"{shown}: a failing oml makes the command exit {run.returncode}, not 0: {command}")


def sh(command: str, path: Path) -> subprocess.CompletedProcess:
    """`command` under /bin/sh with `path` as the whole PATH and no stdin."""
    return subprocess.run(
        ["/bin/sh", "-c", command],
        env={"PATH": str(path)},
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=10,
        check=False,
    )


def check_admin(entries: dict, workspace_version: str, errors: list[str]) -> None:
    if "oml-admin" not in entries:
        errors.append("marketplace.json does not list the oml-admin plugin")
        return
    plugin_dir = (ROOT / entries["oml-admin"]["source"]).resolve()
    manifest_path = plugin_dir / ".claude-plugin/plugin.json"
    if not manifest_path.is_file():
        errors.append(f"{manifest_path} is missing")
        return
    manifest = json.loads(manifest_path.read_text())
    if manifest["version"] != workspace_version:
        errors.append(
            f"oml-admin plugin.json version {manifest['version']} != workspace version {workspace_version}"
        )
    if (plugin_dir / ".mcp.json").exists():
        errors.append("oml-admin has an .mcp.json: an admin plugin serves no MCP tool (ADR-0028 d3)")

    skill_path = plugin_dir / ADMIN_SKILL
    if not skill_path.is_file():
        errors.append(f"{skill_path} is missing")
        return
    runs = [c for c in commands(skill_path.read_text()) if re.search(r"\boml\b", c) and "issue-agent" in c]
    if not runs:
        errors.append(f"{ADMIN_SKILL} runs no `oml keys issue-agent` command")
    for run in runs:
        flags = set(re.findall(r"(?<!\S)(--[a-z-]+)", run))
        if "--out" not in flags:
            errors.append(f"{ADMIN_SKILL} runs issue-agent without --out: {run.strip()}")
        if "--json" in flags:
            errors.append(f"{ADMIN_SKILL} runs issue-agent with --json: {run.strip()}")


def commands(text: str) -> list[str]:
    """Every line of every fenced block, backslash continuations joined, and
    every inline code span outside the blocks."""
    lines = [line for block in FENCE.findall(text) for line in block.replace("\\\n", " ").splitlines()]
    return lines + SPAN.findall(FENCE.sub("", text))


def report(errors: list[str]) -> int:
    for e in errors:
        print(f"plugin check: {e}", file=sys.stderr)
    if not errors:
        print(
            "plugin check: the oml plugin agrees with the server, its SessionStart hook is guarded, "
            "and oml-admin issues keys only with --out"
        )
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
