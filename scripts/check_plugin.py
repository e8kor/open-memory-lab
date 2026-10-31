"""The agent plugin agrees with the server it connects to.

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
  served tool is declared in docs/api/mcp-tools.json.
"""

import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


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

    return report(errors)


def report(errors: list[str]) -> int:
    for e in errors:
        print(f"plugin check: {e}", file=sys.stderr)
    if not errors:
        print("plugin check: the oml plugin agrees with the server")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
