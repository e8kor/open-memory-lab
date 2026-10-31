"""The agent plugin agrees with the server it connects to, and the admin plugin
issues keys only the way ADR-0028 d3 allows.

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
- the marketplace lists oml-admin at a directory that has a manifest, at the
  workspace version;
- oml-admin has no .mcp.json, so it adds no tool to any tool list;
- every command in the oml-admin skill that runs `issue-agent` carries `--out`
  and never `--json`, so the key value never reaches the model's context
  (docs/cli/reference.md, "The oml-admin plugin"), and names its key right
  after `issue-agent` with a name the creation grammar admits (ADR-0026 d2,
  d12, d14);
- every `--workarea` a command on a plugin page passes is a workarea name or
  the placeholder `<workarea>`, never an id, so the pages show the name an
  operator types (ADR-0026 d4, d14).
"""

import json
import re
import shlex
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ADMIN_SKILL = "skills/issue-agent-key/SKILL.md"
# A fenced block, indented or not, and an inline code span outside one.
FENCE = re.compile(r"^[ \t]*```[^\n]*\n(.*?)^[ \t]*```", re.S | re.M)
SPAN = re.compile(r"`([^`\n]+)`")
# A name, as docs/api/http-api.md "Names and ids" declares it (ADR-0026 d2):
# a lowercase RFC 1035 label that is neither the hex-and-dash id form nor 32
# hexadecimal digits; the creation grammar also refuses the derived form.
LABEL = re.compile(r"[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?")
ID_FORM = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
HEX32 = re.compile(r"[0-9a-f]{32}")
DERIVED = re.compile(r"(?:tenant|wa|crew|key)-[0-9a-f]{32}")


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
        check_workarea_flags(doc, text, errors)
        named = set(re.findall(r"`(oml_[a-z_]+)`", text))
        for tool in sorted(served - named):
            errors.append(f"{doc.relative_to(ROOT)} does not name the served tool {tool}")
        unbuilt_mentioned = named - served
        if doc.name == "SKILL.md" and unbuilt_mentioned:
            errors.append(
                f"{doc.relative_to(ROOT)} names tools the server does not serve: {sorted(unbuilt_mentioned)}"
            )

    check_admin(entries, workspace_version, errors)
    return report(errors)


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
    skill = skill_path.read_text()
    check_workarea_flags(skill_path, skill, errors)
    runs = [c for c in commands(skill) if re.search(r"\boml\b", c) and "issue-agent" in c]
    if not runs:
        errors.append(f"{ADMIN_SKILL} runs no `oml keys issue-agent` command")
    for run in runs:
        flags = set(re.findall(r"(?<!\S)(--[a-z-]+)", run))
        if "--out" not in flags:
            errors.append(f"{ADMIN_SKILL} runs issue-agent without --out: {run.strip()}")
        if "--json" in flags:
            errors.append(f"{ADMIN_SKILL} runs issue-agent with --json: {run.strip()}")
        words = tokens(run)
        after = words[words.index("issue-agent") + 1 :] if "issue-agent" in words else []
        if not after or not is_name(after[0], creation=True):
            errors.append(
                f"{ADMIN_SKILL} runs issue-agent without a key name the creation grammar admits "
                f"right after it (ADR-0026 d2, d12): {run.strip()}"
            )


def check_workarea_flags(page: Path, text: str, errors: list[str]) -> None:
    for command in commands(text):
        words = tokens(command)
        values = [b for a, b in zip(words, words[1:]) if a == "--workarea"]
        values += [w.split("=", 1)[1] for w in words if w.startswith("--workarea=")]
        for value in values:
            if value != "<workarea>" and not is_name(value):
                errors.append(
                    f"{page.relative_to(ROOT)} passes --workarea {value}, not a workarea name or "
                    f"<workarea> (ADR-0026 d4): {command.strip()}"
                )


def is_name(value: str, creation: bool = False) -> bool:
    """Whether `value` is a name under ADR-0026 d2's reference grammar, or,
    with `creation`, under its creation grammar."""
    if not LABEL.fullmatch(value) or ID_FORM.fullmatch(value) or HEX32.fullmatch(value):
        return False
    return not (creation and DERIVED.fullmatch(value))


def tokens(command: str) -> list[str]:
    """A command's words as a POSIX shell splits them; on a quote the shell
    could not close, split on whitespace instead."""
    try:
        return shlex.split(command)
    except ValueError:
        return command.split()


def commands(text: str) -> list[str]:
    """Every line of every fenced block, backslash continuations joined, and
    every inline code span outside the blocks."""
    lines = [line for block in FENCE.findall(text) for line in block.replace("\\\n", " ").splitlines()]
    return lines + SPAN.findall(FENCE.sub("", text))


def report(errors: list[str]) -> int:
    for e in errors:
        print(f"plugin check: {e}", file=sys.stderr)
    if not errors:
        print("plugin check: the oml plugin agrees with the server, and oml-admin issues keys only with --out")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
