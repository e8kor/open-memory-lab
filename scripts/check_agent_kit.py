"""The release's agent artifacts package what the repository holds, and the
kit's configurations name no reachable server: each reads OML_URL in its
tool's own syntax or carries a placeholder that never resolves, with a bearer
key from the environment (ADR-0076 d7).

Run by .github/workflows/ci.yml (job plugin). Builds every artifact
scripts/package_agent_artifacts.py writes into a temporary directory, reads
each archive back, and fails with every disagreement found:

- oml-claude-plugins-X.Y.Z.tar.gz and .zip hold the same files, and their
  marketplace lists oml and oml-admin at relative sources that resolve, inside
  the archive, to a directory with a .claude-plugin/plugin.json, so adding the
  extracted directory installs offline;
- oml-marketplace-X.Y.Z.json lists the same plugins, each a git-subdir of the
  repository at the path the repository's marketplace names and at the ref
  vX.Y.Z;
- oml-agent-kit-X.Y.Z.tar.gz and .zip hold the same files: README.md, AGENTS.md
  and one configuration each for Codex, Cursor, Gemini CLI, VS Code and Claude
  Desktop;
- every configuration declares exactly one server, oml, whose URL ends in
  /mcp, and whose bearer key is OML_API_KEY taken from the environment in that
  tool's documented form; no file in either archive holds a literal key;
- Cursor reads ${env:OML_URL}/mcp and Gemini CLI ${OML_URL}/mcp; OpenAI Codex
  CLI, VS Code and Claude Desktop carry https://oml-server.invalid/mcp, a
  name that never resolves (RFC 6761), so no configuration names a server
  (ADR-0076 d7, d10 test 3);
- the kit README's "Before you start", ahead of the copy steps, exports
  OML_API_KEY and OML_URL without /mcp, and names the placeholder edit for
  Codex CLI, VS Code and Claude Desktop in one paragraph;
- the AGENTS.md block is, section for section, the text of the oml plugin's
  skill;
- the Gemini CLI SessionStart hook runs the oml plugin's own SessionStart
  command, which check_plugin.py holds to ADR-0029 d7's guard.

Dependencies: stdlib.
"""

import json
import re
import sys
import tarfile
import tempfile
import tomllib
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import package_agent_artifacts as pkg  # noqa: E402

KEY = pkg.KEY_VAR
# An OML key's shape: `oml_` and a long run of key characters. A tool name
# such as oml_workarea breaks the run at its underscores.
LITERAL_KEY = re.compile(r"\boml_[A-Za-z0-9-]{16,}")
# The placeholder of the tools that expand no variable inside a URL, and
# each tool's own form of OML_URL where it does (ADR-0076 d7). Written out
# here, not read from package_agent_artifacts.py, so the check does not agree
# with itself.
PLACEHOLDER = "https://oml-server.invalid/mcp"
EXPECTED_URL = {
    "codex/config.toml": PLACEHOLDER,
    "cursor/.cursor/mcp.json": "${env:OML_URL}/mcp",
    "gemini/.gemini/settings.json": "${OML_URL}/mcp",
    "vscode/.vscode/mcp.json": PLACEHOLDER,
    "claude-desktop/claude_desktop_config.json": PLACEHOLDER,
}
# The kit README's name for each configuration's tool.
TOOL_NAME = {
    "codex/config.toml": "OpenAI Codex CLI",
    "cursor/.cursor/mcp.json": "Cursor",
    "gemini/.gemini/settings.json": "Gemini CLI",
    "vscode/.vscode/mcp.json": "VS Code",
    "claude-desktop/claude_desktop_config.json": "Claude Desktop",
}
KIT_CONFIGS = {
    "codex/config.toml",
    "cursor/.cursor/mcp.json",
    "gemini/.gemini/settings.json",
    "vscode/.vscode/mcp.json",
    "claude-desktop/claude_desktop_config.json",
}


def main() -> int:
    errors: list[str] = []
    version = pkg.workspace_version()
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        pkg.package(out, version, pkg.DEFAULT_REPO, "0" * 40)
        plugins = archive_pair(errors, out, f"oml-claude-plugins-{version}")
        kit = archive_pair(errors, out, f"oml-agent-kit-{version}")
        pinned = json.loads((out / f"oml-marketplace-{version}.json").read_text())
    for name, files in (("plugins", plugins), ("kit", kit)):
        for path, data in files.items():
            if LITERAL_KEY.search(data.decode("utf-8", "replace")):
                errors.append(f"{name} archive: {path} holds what looks like a literal key")
    check_plugins(errors, plugins)
    check_pinned(errors, pinned, version)
    check_kit(errors, kit)
    return report(errors)


def archive_pair(errors: list[str], out: Path, stem: str) -> dict:
    """The files of `stem`.tar.gz, relative to `stem`/, after checking that
    `stem`.zip holds the same ones."""
    tgz: dict = {}
    with tarfile.open(out / f"{stem}.tar.gz") as tar:
        for member in tar.getmembers():
            if member.isfile():
                tgz[member.name] = tar.extractfile(member).read()
    zipped: dict = {}
    with zipfile.ZipFile(out / f"{stem}.zip") as z:
        for info in z.infolist():
            if not info.is_dir():
                zipped[info.filename] = z.read(info)
    if tgz != zipped:
        errors.append(f"{stem}: the .tar.gz and the .zip hold different files")
    files = {}
    for name, data in tgz.items():
        if not name.startswith(f"{stem}/"):
            errors.append(f"{stem}.tar.gz: {name} is not under {stem}/")
            continue
        files[name[len(stem) + 1 :]] = data
    return files


def check_plugins(errors: list[str], files: dict) -> None:
    where = "oml-claude-plugins"
    raw = files.get(".claude-plugin/marketplace.json")
    if raw is None:
        errors.append(f"{where}: no .claude-plugin/marketplace.json at the archive's root")
        return
    marketplace = json.loads(raw)
    names = [p["name"] for p in marketplace["plugins"]]
    if sorted(names) != ["oml", "oml-admin"]:
        errors.append(f"{where}: the marketplace lists {names}, not oml and oml-admin")
    for entry in marketplace["plugins"]:
        source = entry["source"]
        if not (isinstance(source, str) and source.startswith("./")) or ".." in source:
            errors.append(f"{where}: {entry['name']}'s source {source!r} is not a path inside the archive")
            continue
        manifest = files.get(f"{source[2:]}/.claude-plugin/plugin.json")
        if manifest is None:
            errors.append(f"{where}: {entry['name']}'s source {source} has no .claude-plugin/plugin.json")
        elif json.loads(manifest)["name"] != entry["name"]:
            errors.append(f"{where}: {source}/.claude-plugin/plugin.json names another plugin than {entry['name']}")
    if "LICENSE" not in files:
        errors.append(f"{where}: no LICENSE")


def check_pinned(errors: list[str], pinned: dict, version: str) -> None:
    where = f"oml-marketplace-{version}.json"
    repo = json.loads(pkg.MARKETPLACE.read_text())
    expected = {p["name"]: p["source"][2:] for p in repo["plugins"]}
    got = {}
    for entry in pinned["plugins"]:
        source = entry["source"]
        if not isinstance(source, dict) or source.get("source") != "git-subdir":
            errors.append(f"{where}: {entry['name']} is not a git-subdir source")
            continue
        if source.get("ref") != f"v{version}":
            errors.append(f"{where}: {entry['name']} is pinned to {source.get('ref')!r}, not v{version}")
        if source.get("url") != f"https://github.com/{pkg.DEFAULT_REPO}.git":
            errors.append(f"{where}: {entry['name']} fetches from {source.get('url')!r}")
        got[entry["name"]] = source.get("path")
    if got != expected:
        errors.append(f"{where}: lists {got}, not the repository marketplace's {expected}")
    if pinned.get("name") != repo.get("name"):
        errors.append(f"{where}: is named {pinned.get('name')!r}, not {repo.get('name')!r}")


def check_kit(errors: list[str], files: dict) -> None:
    for path in sorted({"README.md", "AGENTS.md"} | KIT_CONFIGS):
        if path not in files:
            errors.append(f"oml-agent-kit: {path} is missing")
    extra = set(files) - KIT_CONFIGS - {"README.md", "AGENTS.md"}
    if extra:
        errors.append(f"oml-agent-kit: holds files no check covers: {sorted(extra)}")
    if any(path not in files for path in KIT_CONFIGS):
        return
    text = {path: files[path].decode() for path in KIT_CONFIGS}

    codex = tomllib.loads(text["codex/config.toml"])["mcp_servers"]
    server_checks(errors, "codex/config.toml", codex, url_field="url")
    if codex.get("oml", {}).get("bearer_token_env_var") != KEY:
        errors.append(f"codex/config.toml: bearer_token_env_var is not {KEY}")
    if set(codex.get("oml", {})) - {"url", "bearer_token_env_var"}:
        errors.append("codex/config.toml: the oml table has fields beyond url and bearer_token_env_var")

    for path, top, url_field, header in (
        ("cursor/.cursor/mcp.json", "mcpServers", "url", f"Bearer ${{env:{KEY}}}"),
        ("vscode/.vscode/mcp.json", "servers", "url", f"Bearer ${{env:{KEY}}}"),
        ("gemini/.gemini/settings.json", "mcpServers", "httpUrl", f"Bearer ${{{KEY}}}"),
    ):
        servers = json.loads(text[path])[top]
        server_checks(errors, path, servers, url_field=url_field)
        got = servers.get("oml", {}).get("headers", {})
        if got != {"Authorization": header}:
            errors.append(f"{path}: headers are {got}, not exactly Authorization: {header}")
    if json.loads(text["vscode/.vscode/mcp.json"])["servers"].get("oml", {}).get("type") != "http":
        errors.append("vscode/.vscode/mcp.json: the oml server's type is not http")

    desktop = json.loads(text["claude-desktop/claude_desktop_config.json"])["mcpServers"]
    if list(desktop) != ["oml"]:
        errors.append(f"claude_desktop_config.json: declares {sorted(desktop)}, not exactly oml")
    args = desktop.get("oml", {}).get("args", [])
    if not any(str(a).endswith("/mcp") for a in args):
        errors.append("claude_desktop_config.json: no argument is a URL ending in /mcp")
    if args[-2:] != ["--header", f"Authorization: Bearer ${{{KEY}}}"]:
        errors.append(f"claude_desktop_config.json: the bridge's header is not Bearer ${{{KEY}}}: {args}")
    if "env" in desktop.get("oml", {}):
        errors.append("claude_desktop_config.json: the oml entry sets env, where a key would be literal")

    every_configuration_names_no_server(errors, text)
    the_kit_readme_names_the_url_edit_before_the_copy_steps(errors, files["README.md"].decode())
    check_agents(errors, files["AGENTS.md"].decode())
    check_gemini_hook(errors, json.loads(text["gemini/.gemini/settings.json"]))


def server_checks(errors: list[str], path: str, servers: dict, url_field: str) -> None:
    if list(servers) != ["oml"]:
        errors.append(f"{path}: declares {sorted(servers)}, not exactly the one server oml")
    url = str(servers.get("oml", {}).get(url_field, ""))
    if url != EXPECTED_URL.get(path, url) or not url.endswith("/mcp"):
        errors.append(f"{path}: {url_field} {url!r} is not {EXPECTED_URL.get(path)!r}")


def config_url(path: str, text: str) -> str | None:
    """The one URL a configuration sends to, in the field its tool reads."""
    if path == "codex/config.toml":
        return tomllib.loads(text)["mcp_servers"].get("oml", {}).get("url")
    if path == "claude-desktop/claude_desktop_config.json":
        args = json.loads(text)["mcpServers"].get("oml", {}).get("args", [])
        urls = [a for a in args if str(a).endswith("/mcp")]
        return urls[0] if len(urls) == 1 else None
    top, field = {
        "cursor/.cursor/mcp.json": ("mcpServers", "url"),
        "gemini/.gemini/settings.json": ("mcpServers", "httpUrl"),
        "vscode/.vscode/mcp.json": ("servers", "url"),
    }[path]
    return json.loads(text)[top].get("oml", {}).get(field)


def every_configuration_names_no_server(errors: list[str], text: dict) -> None:
    """Each configuration reads OML_URL in its tool's own syntax or carries
    the placeholder, so none names a server (ADR-0076 d7)."""
    for path, expected in sorted(EXPECTED_URL.items()):
        got = config_url(path, text[path])
        if got != expected:
            errors.append(f"{path}: the server URL is {got!r}, not {expected!r} (ADR-0076 d7)")


def readme_section(readme: str, heading: str) -> str:
    match = re.search(rf"^## {re.escape(heading)}\n(.*?)(?=^## |\Z)", readme, re.S | re.M)
    return match.group(1) if match else ""


def the_kit_readme_names_the_url_edit_before_the_copy_steps(errors: list[str], readme: str) -> None:
    where = "kit README"
    start = readme_section(readme, "Before you start")
    if not start:
        errors.append(f"{where}: has no 'Before you start' section")
        return
    if readme.find("## Before you start") > readme.find("## The tools"):
        errors.append(f"{where}: 'Before you start' does not come before 'The tools'")
    if not re.search(rf"^export {KEY}=", start, re.M):
        errors.append(f"{where}: 'Before you start' does not export {KEY}")
    if not re.search(r"^export OML_URL=", start, re.M):
        errors.append(f"{where}: 'Before you start' does not export OML_URL, which Cursor and Gemini CLI read")
    edits = [p for p in re.split(r"\n\s*\n", start) if PLACEHOLDER in p]
    if len(edits) != 1:
        errors.append(f"{where}: 'Before you start' names {PLACEHOLDER} in {len(edits)} paragraphs, not one")
        return
    for path, expected in sorted(EXPECTED_URL.items()):
        if expected != PLACEHOLDER:
            continue
        tool = TOOL_NAME[path]
        if re.search(rf"(?<![\w-]){re.escape(tool)}(?![\w-])", edits[0]) is None:
            errors.append(f"{where}: the placeholder edit does not name {tool}")
    if re.search(r"OML_URL=\S*/mcp\b", start):
        errors.append(f"{where}: exports OML_URL with /mcp, which the CLI and the hook append themselves")


def check_agents(errors: list[str], text: str) -> None:
    """Every section the block carries is the skill's section, verbatim, in order."""
    block = re.search(re.escape(pkg.AGENTS_BEGIN) + r"\n\n(.*)\n\n" + re.escape(pkg.AGENTS_END), text, re.S)
    if not block:
        errors.append("AGENTS.md: no block between the oml:begin and oml:end markers")
        return
    skill = pkg.skill_sections()
    expected = "\n\n".join(skill[s] for s in pkg.AGENTS_SECTIONS if s in skill)
    if block.group(1) != expected:
        errors.append("AGENTS.md: the block's text is not the oml-memory skill's sections")
    for tool in ("oml_write", "oml_read", "oml_workarea"):
        if f"`{tool}`" not in block.group(1):
            errors.append(f"AGENTS.md: the block never names {tool}")


def check_gemini_hook(errors: list[str], settings: dict) -> None:
    plugin = json.loads(pkg.HOOKS.read_text())["hooks"]["SessionStart"][0]["hooks"][0]["command"]
    groups = settings.get("hooks", {}).get("SessionStart", [])
    hooks = [h for g in groups for h in g.get("hooks", [])]
    if [h.get("command") for h in hooks] != [plugin]:
        errors.append(f"gemini settings.json: SessionStart runs {[h.get('command') for h in hooks]}, not the plugin's {plugin!r}")
    if any("matcher" in g for g in groups):
        errors.append("gemini settings.json: the SessionStart group has a matcher; the hook runs for every source")
    if set(settings.get("hooks", {})) != {"SessionStart"}:
        errors.append(f"gemini settings.json: wires hooks {sorted(settings.get('hooks', {}))}, not SessionStart alone")


def report(errors: list[str]) -> int:
    for e in errors:
        print(f"agent kit check: {e}", file=sys.stderr)
    if not errors:
        print(
            "agent kit check: the plugin archive installs offline, the pinned marketplace names the tag,"
            " and every kit configuration reaches the default server's /mcp with OML_API_KEY from the"
            " environment"
        )
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
