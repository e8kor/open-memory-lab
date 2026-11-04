"""The agent plugin agrees with the server it connects to, its SessionStart hook
is guarded the way ADR-0029 d7 decides, and the admin plugin issues keys only
the way ADR-0028 d3 allows.

Run by .github/workflows/ci.yml (job plugin). Checks, failing on the first
class of disagreement found:

- the marketplace lists the plugin at a directory that has a manifest;
- the plugin's version is the workspace version (Cargo.toml), so a release tag
  names one version for the image, the binaries and the plugin;
- the plugin declares exactly url and a sensitive api_key: plugin.json's
  userConfig is ADR-0040 d1's two options, titles and descriptions included,
  neither with a default;
- the mcp entry reads no environment variable: .mcp.json declares exactly one
  http server, `oml`, since one key does the agent's whole loop (ADR-0025), at
  `${user_config.url}/mcp` with the header `Bearer ${user_config.api_key}`,
  and names no `OML_` variable (ADR-0040 d2);
- the plugin README shows no `export OML_API_KEY` line: the Claude Code path
  configures the api_key option instead (ADR-0040 d4, d6);
- the skill and the plugin README name every tool the server serves
  (SERVED_TOOLS in crates/transports/oml-transport-http/src/mcp.rs), every
  served tool is declared in docs/api/mcp-tools.json, and the skill, its
  description included, and the lines the session-start hook adds to a
  session (crates/bins/oml-cli/src/hook.rs) name no other tool, so the model
  is never told to call a name tools/list does not offer (ADR-0039 d2, d13);
- every permission rule the plugin README shows, `mcp__plugin_oml_oml__<tool>`,
  names a served tool (ADR-0039 d13, amending ADR-0029 d8);
- plugins/oml/hooks/hooks.json exists and parses, and its SessionStart hooks
  are exactly one command, in a group with no matcher, with a timeout of 10
  seconds and the status message "Loading OML memory", which, run under
  /bin/sh, exits 0 with no output when no `oml` is on PATH, and otherwise
  runs `oml` with exactly the arguments `hook session-start` and exits 0 even
  when that `oml` fails (docs/cli/reference.md, "oml hook session-start";
  ADR-0029 d7);
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
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ADMIN_SKILL = "skills/issue-agent-key/SKILL.md"
HOOKS = "hooks/hooks.json"
# Above the verb's own 8-second deadline (ADR-0029 d5, d7).
HOOK_TIMEOUT = 10
HOOK_STATUS = "Loading OML memory"
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
# A tool name, bare or backticked. A key literal such as oml_k_01JB… or
# oml_... is not one: its run breaks at a digit, a capital or a dot.
TOOL = re.compile(r"\boml_[a-z]+(?:_[a-z]+)*\b")
RULE = re.compile(r"mcp__plugin_oml_oml__([a-z_]+)")
HOOK_RS = "crates/bins/oml-cli/src/hook.rs"
# ADR-0040 d1: the same URL-and-key pair as every other client, lower-cased.
USER_CONFIG = {
    "url": {
        "type": "string",
        "title": "OML server URL",
        "description": "Your Open Memory Lab server, without /mcp, for example https://oml.example.com",
        "required": True,
    },
    "api_key": {
        "type": "string",
        "title": "Agent key",
        "description": "This agent's own OML key, never an admin key. Stored in the system keychain.",
        "sensitive": True,
        "required": True,
    },
}
# ADR-0040 d2.
MCP_URL = "${user_config.url}/mcp"
MCP_AUTHORIZATION = "Bearer ${user_config.api_key}"
EXPORT_KEY = re.compile(r"^\s*(?:\$\s*)?export\s+OML_API_KEY\b")


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

    the_plugin_declares_exactly_url_and_a_sensitive_api_key(manifest, errors)
    the_mcp_entry_reads_no_environment_variable(plugin_dir, errors)
    the_readme_exports_no_api_key(plugin_dir, errors)

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
        if doc.name == "README.md":
            for tool in sorted(set(RULE.findall(text)) - served):
                errors.append(
                    f"{doc.relative_to(ROOT)} shows the permission rule mcp__plugin_oml_oml__{tool}, "
                    f"which names no served tool (ADR-0039 d13)"
                )

    for path in [plugin_dir / "skills/oml-memory/SKILL.md", ROOT / HOOK_RS]:
        unserved = set(TOOL.findall(path.read_text())) - served
        if unserved:
            errors.append(
                f"{path.relative_to(ROOT)} names tools the server does not serve: {sorted(unserved)}"
            )

    check_hooks(plugin_dir, errors)
    check_admin(entries, workspace_version, errors)
    return report(errors)


def the_plugin_declares_exactly_url_and_a_sensitive_api_key(manifest: dict, errors: list[str]) -> None:
    """plugin.json's userConfig is exactly ADR-0040 d1's two options: `url`, and
    `api_key` with `sensitive: true`, so Claude Code keeps the key in the
    keychain and never in settings.json; neither has a default, which would
    send the key to whatever listens there."""
    declared = manifest.get("userConfig")
    if declared != USER_CONFIG:
        errors.append(
            f"plugin.json userConfig is {json.dumps(declared, sort_keys=True)}, not exactly the url and "
            f"sensitive api_key options of ADR-0040 d1: {json.dumps(USER_CONFIG, sort_keys=True)}"
        )


def the_mcp_entry_reads_no_environment_variable(plugin_dir: Path, errors: list[str]) -> None:
    """.mcp.json declares the one http server `oml` (ADR-0025), at
    `${user_config.url}/mcp` with the header `Bearer ${user_config.api_key}`,
    and names no `OML_` variable, so the tools and the hook never read two
    different keys (ADR-0040 d2)."""
    text = (plugin_dir / ".mcp.json").read_text()
    servers = json.loads(text)["mcpServers"]
    if list(servers) != ["oml"]:
        errors.append(f".mcp.json declares {sorted(servers)}, not exactly the one server oml")
    for name, server in servers.items():
        if server.get("type") != "http":
            errors.append(f".mcp.json {name}: type is {server.get('type')!r}, not http")
        if server.get("url") != MCP_URL:
            errors.append(f".mcp.json {name}: url is {server.get('url')!r}, not {MCP_URL!r} (ADR-0040 d2)")
        if server.get("headers") != {"Authorization": MCP_AUTHORIZATION}:
            errors.append(
                f".mcp.json {name}: headers are {server.get('headers')!r}, not exactly "
                f"Authorization {MCP_AUTHORIZATION!r} (ADR-0040 d2)"
            )
    if "OML_" in text:
        errors.append(".mcp.json names an OML_ variable; the MCP entry reads the plugin's options alone (ADR-0040 d2)")


def the_readme_exports_no_api_key(plugin_dir: Path, errors: list[str]) -> None:
    """The plugin README configures the key through Claude Code and shows no
    `export OML_API_KEY` line, so no example reads a credential from the
    user's environment and sends it to a server (ADR-0040 d4, d6)."""
    readme = plugin_dir / "README.md"
    for number, line in enumerate(readme.read_text().splitlines(), 1):
        if EXPORT_KEY.search(line):
            errors.append(
                f"{readme.relative_to(ROOT)}:{number} shows `export OML_API_KEY`; the Claude Code path "
                f"configures the api_key option (ADR-0040 d4, d6): {line.strip()}"
            )


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
    entry = entries[0]
    if any("matcher" in group for group in groups):
        errors.append(f"{shown}: the SessionStart group has a matcher; the hook runs for every source")
    if entry.get("timeout") != HOOK_TIMEOUT:
        errors.append(f"{shown}: the SessionStart timeout is {entry.get('timeout')!r}, not {HOOK_TIMEOUT}")
    if entry.get("statusMessage") != HOOK_STATUS:
        errors.append(f"{shown}: the SessionStart statusMessage is {entry.get('statusMessage')!r}, not {HOOK_STATUS!r}")
    command = entry.get("command", "")

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
        print(
            "plugin check: the oml plugin agrees with the server, its SessionStart hook is guarded, "
            "and oml-admin issues keys only with --out"
        )
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
