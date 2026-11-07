"""The agent plugin agrees with the server it connects to, its hooks are
guarded the way ADR-0029 d7 and ADR-0038 d9 decide, and the admin plugin
issues keys only the way ADR-0028 d3 allows.

Run by .github/workflows/ci.yml (job plugin). Checks, failing on the first
class of disagreement found:

- the marketplace lists the plugin at a directory that has a manifest;
- the plugin's version is the workspace version (Cargo.toml), so a release tag
  names one version for the image, the binaries and the plugin;
- the plugin declares exactly url and a sensitive api_key: plugin.json's
  userConfig is ADR-0040 d1's two options, titles and descriptions included,
  as ADR-0043 d2 amends them: url with the default server and not required,
  api_key required with no default; and url's default is DEFAULT_URL in
  crates/bins/oml-cli/src/setup.rs, the server the hooks and `oml setup
  claude-code` take for a key named alone (ADR-0043 d1, d8);
- the mcp entry reads no environment variable: .mcp.json declares exactly one
  http server, `oml`, since one key does the agent's whole loop (ADR-0025), at
  `${user_config.url}/mcp` with the header `Bearer ${user_config.api_key}`,
  and names no `OML_` variable (ADR-0040 d2);
- the plugin README shows no `export OML_API_KEY` line: the Claude Code path
  configures the api_key option instead (ADR-0040 d4, d6);
- the skill and the plugin README name every tool the server serves
  (SERVED_TOOLS in crates/transports/oml-transport-http/src/mcp.rs), every
  served tool is declared in docs/api/mcp-tools.json, and the skill, its
  description included, and the lines the hooks add to a session (the code
  of crates/bins/oml-cli/src/hook.rs above its unit tests) name no other tool, so the model
  is never told to call a name tools/list does not offer (ADR-0039 d2, d13);
- every permission rule the plugin README shows, `mcp__plugin_oml_oml__<tool>`,
  names a served tool (ADR-0039 d13, amending ADR-0029 d8);
- plugins/oml/hooks/hooks.json exists and parses, declares exactly the
  events SessionStart, SubagentStart, PreToolUse, PostToolUseFailure and
  SessionEnd, and each group holds exactly one command: SessionStart with no matcher, a timeout
  of 10 seconds and the status message "Loading OML memory", running
  `hook session-start` (ADR-0029 d7); PostToolUseFailure with the matcher
  `Bash` and a timeout of 3 seconds, running `hook tool-failure`; SessionEnd
  with no matcher and a timeout of 1 second, running `hook session-end`
  (ADR-0038 d9). Each command, run under /bin/sh, exits 0 with no output
  when no `oml` is on PATH, and otherwise runs `oml` with exactly its
  arguments and exits 0 even when that `oml` fails (docs/cli/reference.md,
  "oml hook session-start" and "oml hook tool-failure");
- the guardrail hooks (proposed ADR-0037 d6, d9): SessionStart holds a second
  group and SubagentStart one, each with no matcher, a timeout of 10 seconds
  and the status message "Loading OML guardrails", running `hook
  guardrails`, guarded as above; PreToolUse holds one group with the matcher
  of d9's tools, a timeout of 5 seconds, onFailure "block" and the status
  message "Checking OML guardrails", running `hook pre-tool-use`, whose
  command exits 0 in silence with no `oml` and with one older than the verb,
  and exits non-zero when the verb fails, so a crash blocks the call;
- plugins/oml/agents/ holds exactly one subagent, librarian.md, whose
  frontmatter is one plain-scalar `field: value` per line (no value starting
  with a YAML indicator, holding ': ' or ' #', ending in ':' or holding a
  tab), since frontmatter that does not parse loads the agent with every
  field ignored and its tools inherited, and which names it `librarian`, sets `model: haiku`, sets only fields a
  plugin subagent supports, and allows exactly the one tool
  `mcp__plugin_oml_oml__oml_read`, so it reads and never writes: no
  `oml_write`, no `oml_workarea`, no built-in tool that edits a file or runs a
  command; its prompt names no tool the server does not serve, sets the
  framing line of hook.rs's FRAMING over what it returns, and the plugin
  README names it `oml:librarian` (ADR-0038 d10);
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
# Each event the plugin hooks: (matcher, timeout in seconds, status message,
# the arguments `oml` runs with). SessionStart's timeout is above the verb's
# 8-second deadline (ADR-0029 d5, d7); PostToolUseFailure's above its
# 2-second deadline, and SessionEnd's under the 1.5 seconds Claude Code
# gives a plugin's SessionEnd hooks in all (ADR-0038 d7, d9).
# Each event holds these groups, in order, each one command; a guarded
# command exits 0 whatever `oml` does, and the checked one, the guardrail
# check, exits 0 only when `oml` is missing or older than its verb, so a
# failing check blocks the call (proposed ADR-0037 d6, d9).
GUARDRAIL_MATCHER = "^(Bash|PowerShell|Write|.*Edit|Read|Grep|Glob|WebFetch|mcp__.*)$"
POST_TOOL_MATCHER = "^(Write|Edit|MultiEdit|NotebookEdit|Read)$"
HOOK_EVENTS = {
    "SessionStart": [
        (None, 10, "Loading OML memory", ["hook", "session-start"], "guarded"),
        (None, 10, "Loading OML guardrails", ["hook", "guardrails"], "guarded"),
    ],
    "SubagentStart": [
        (None, 10, "Loading OML guardrails", ["hook", "guardrails"], "guarded"),
    ],
    "PreToolUse": [
        (GUARDRAIL_MATCHER, 5, "Checking OML guardrails", ["hook", "pre-tool-use"], "checked"),
    ],
    "PostToolUseFailure": [("Bash", 3, None, ["hook", "tool-failure"], "guarded")],
    # ADR-0064 d11, d13: records the paths an edit or a read touched; sends
    # and prints nothing, and does nothing in a clone not set up for it.
    "PostToolUse": [(POST_TOOL_MATCHER, 2, None, ["hook", "post-tool-use"], "guarded")],
    "SessionEnd": [(None, 1, None, ["hook", "session-end"], "guarded")],
    # ADR-0063 d6, d10: each above its verb's deadline (2 s for user-prompt,
    # 1 s for stop and pre-compact), each a no-op until switched on.
    "UserPromptSubmit": [(None, 3, None, ["hook", "user-prompt"], "guarded")],
    "Stop": [(None, 2, None, ["hook", "stop"], "guarded")],
    "PreCompact": [(None, 2, None, ["hook", "pre-compact"], "guarded")],
}
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
# ADR-0038 d10: the plugin's one subagent and the one tool it may call.
LIBRARIAN = "agents/librarian.md"
LIBRARIAN_TOOLS = ["mcp__plugin_oml_oml__oml_read"]
# The frontmatter fields a plugin subagent supports (Claude Code, "Add
# components to a plugin", "Frontmatter fields in plugin agents", fetched
# 2026-11-04); permissionMode, hooks, mcpServers and initialPrompt are
# ignored there, so a librarian setting one would read as holding a rule it
# does not.
# The characters a plain YAML scalar may not start with (YAML 1.2, c-indicator);
# `-`, `?` and `:` may, when a non-space follows them.
YAML_INDICATORS = set(",[]{}#&*!|>'\"%@`")
PLUGIN_AGENT_FIELDS = {
    "name", "description", "model", "effort", "maxTurns", "autoCompactWindow", "tools",
    "disallowedTools", "skills", "memory", "background", "omitClaudeMd", "isolation", "color",
}
# ADR-0040 d1: the same URL-and-key pair as every other client, lower-cased;
# ADR-0043 d2: url defaults to the maintainer's public server and is not
# required. Written out here, not read from setup.rs, so the check does not
# agree with itself; the_default_url_is_the_clis checks the two agree.
DEFAULT_URL = "https://memory.doublegate.io"
SETUP_RS = "crates/bins/oml-cli/src/setup.rs"
USER_CONFIG = {
    "url": {
        "type": "string",
        "title": "OML server URL",
        "description": "Your Open Memory Lab server, without /mcp. Leave the default to use the maintainer's public server.",
        "default": DEFAULT_URL,
    },
    "api_key": {
        "type": "string",
        "title": "Agent key",
        "description": "This agent's own OML key, never an admin key. Stored in the system keychain.",
        "sensitive": True,
        "required": True,
    },
    # ADR-0063 d6, d8: where chat capture writes and per-turn recall reads.
    "chat_workarea": {
        "type": "string",
        "title": "Chat workarea",
        "description": "Your chat workarea's id, as oml chat init prints it. Chat capture needs it; per-turn recall without it reads only the key's default workarea.",
        "default": "",
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
    the_default_url_is_the_clis(errors)
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

    for path in [plugin_dir / "skills/oml-memory/SKILL.md", plugin_dir / LIBRARIAN, ROOT / HOOK_RS]:
        if not path.is_file():
            continue
        text = path.read_text()
        # hook.rs's unit tests feed the hooks error lines and URLs, which
        # are never added to a session; only the code above them is held.
        if path.name == "hook.rs":
            text = text.split("#[cfg(test)]")[0]
        unserved = set(TOOL.findall(text)) - served
        if unserved:
            errors.append(
                f"{path.relative_to(ROOT)} names tools the server does not serve: {sorted(unserved)}"
            )

    the_librarian_reads_and_never_writes(plugin_dir, served, errors)
    check_hooks(plugin_dir, errors)
    check_admin(entries, workspace_version, errors)
    return report(errors)


def the_plugin_declares_exactly_url_and_a_sensitive_api_key(manifest: dict, errors: list[str]) -> None:
    """plugin.json's userConfig is exactly ADR-0040 d1's two options as ADR-0043
    d2 amends them: `url` with the maintainer's public server as its default
    and not required, so a user of that server pastes only the key, and
    `api_key` with `sensitive: true` and no default, so Claude Code keeps the
    key in the keychain and never in settings.json."""
    declared = manifest.get("userConfig")
    if declared != USER_CONFIG:
        errors.append(
            f"plugin.json userConfig is {json.dumps(declared, sort_keys=True)}, not exactly the url and "
            f"sensitive api_key options of ADR-0040 d1 and ADR-0043 d2: {json.dumps(USER_CONFIG, sort_keys=True)}"
        )


def the_default_url_is_the_clis(errors: list[str]) -> None:
    """The url option's default is the server `oml hook` pairs with a plugin
    key set alone and `oml setup claude-code` with a key named alone, so the
    tools, the hooks and the setup verb reach one server (ADR-0043 d1, d3, d4)."""
    match = re.search(r'pub const DEFAULT_URL: &str = "([^"]*)";', (ROOT / SETUP_RS).read_text())
    if not match:
        errors.append(f"DEFAULT_URL not found in {SETUP_RS}")
    elif match.group(1) != DEFAULT_URL:
        errors.append(
            f"{SETUP_RS} DEFAULT_URL is {match.group(1)!r}, not the plugin's url default {DEFAULT_URL!r} "
            f"(ADR-0043 d1)"
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


def the_librarian_reads_and_never_writes(plugin_dir: Path, served: set[str], errors: list[str]) -> None:
    """plugins/oml/agents/ holds exactly librarian.md, a haiku subagent whose
    one tool is oml_read, so it answers "what does OML hold about X" without
    a way to write, edit a file or run a command, and returns what it found
    under the framing line every injected context sets (ADR-0038 d10,
    ADR-0029 d6)."""
    agents = plugin_dir / "agents"
    path = plugin_dir / LIBRARIAN
    shown = path.relative_to(ROOT)
    found = sorted(p.relative_to(agents).as_posix() for p in agents.rglob("*") if p.is_file()) if agents.is_dir() else []
    if found != ["librarian.md"]:
        errors.append(f"plugins/oml/agents holds {found}, not exactly librarian.md (ADR-0038 d10)")
    if not path.is_file():
        return
    text = path.read_text()
    match = re.match(r"---\n(.*?)\n---\n(.*)", text, re.S)
    if not match:
        errors.append(f"{shown} has no frontmatter between --- lines")
        return
    fields: dict[str, str] = {}
    for line in match.group(1).splitlines():
        key, sep, value = line.partition(":")
        if not sep or line[:1].isspace():
            errors.append(f"{shown}: frontmatter line {line!r} is not one `field: value` (ADR-0038 d10)")
            continue
        fault = plain_scalar_fault(line, value)
        if fault:
            errors.append(
                f"{shown}: the {key.strip()} line {fault}, so it is not a plain YAML scalar and Claude "
                f"Code would load the agent with every field ignored: the librarian would lose its tools "
                f"field and inherit every tool a subagent has, oml_write, Bash and Edit among them "
                f"(ADR-0038 d10)"
            )
        fields[key.strip()] = value.strip()
    body = match.group(2)
    for field in sorted(set(fields) - PLUGIN_AGENT_FIELDS):
        errors.append(f"{shown} sets {field}, which a plugin subagent does not support (ADR-0038 d10)")
    if fields.get("name") != "librarian":
        errors.append(f"{shown}: name is {fields.get('name')!r}, not 'librarian' (ADR-0038 d10)")
    if fields.get("model") != "haiku":
        errors.append(f"{shown}: model is {fields.get('model')!r}, not 'haiku' (ADR-0038 d10)")
    if not fields.get("description"):
        errors.append(f"{shown} has no description, so Claude never delegates to it")
    tools = [t.strip() for t in fields.get("tools", "").split(",") if t.strip()]
    if tools != LIBRARIAN_TOOLS:
        errors.append(
            f"{shown}: tools are {tools}, not exactly {LIBRARIAN_TOOLS}: the librarian reads and "
            f"never writes, and an omitted tools field would inherit oml_write (ADR-0038 d10)"
        )
    for tool in tools:
        name = tool.removeprefix("mcp__plugin_oml_oml__")
        if name not in served:
            errors.append(f"{shown}: the tool {tool} names no served tool (ADR-0039 d2)")
    hook_rs = (ROOT / HOOK_RS).read_text()
    framing = re.search(r'pub const FRAMING: &str =\s*"([^"]*)";', hook_rs)
    if not framing:
        errors.append(f"FRAMING not found in {HOOK_RS}")
    elif framing.group(1) not in body:
        errors.append(
            f"{shown} does not set the framing line {framing.group(1)!r} over its findings "
            f"(ADR-0029 d6, ADR-0038 d10)"
        )
    if "`oml:librarian`" not in (plugin_dir / "README.md").read_text():
        errors.append("plugins/oml/README.md does not name the subagent `oml:librarian` (ADR-0038 d10)")


def plain_scalar_fault(line: str, value: str) -> str | None:
    """Why `field: value` is not one plain YAML scalar, or None. The check
    reads frontmatter without a YAML parser, so it admits only a subset every
    parser reads as written: a value that cannot start, end or break a plain
    scalar and holds no tab (YAML 1.2, 7.3.3)."""
    if "\t" in line:
        return "holds a tab"
    if value and not value.startswith(" "):
        return "has no space after its ':'"
    stripped = value.strip()
    if stripped[:1] in YAML_INDICATORS:
        return f"starts its value with the indicator {stripped[0]!r}"
    if stripped and stripped[0] in "-?:" and stripped[1:2] in ("", " "):
        return f"starts its value with {stripped[0]!r} and a space or nothing"
    if ": " in stripped or stripped.endswith(":"):
        return "holds ': ' or ends in ':'"
    if " #" in stripped:
        return "holds ' #', which starts a comment"
    return None


def check_hooks(plugin_dir: Path, errors: list[str]) -> None:
    """Each hook runs its `oml hook` verb only when `oml` is on PATH, and a
    missing or failing `oml` exits 0, so a user without the CLI, or with a
    CLI older than the verb, sees no hook error (ADR-0029 d7, ADR-0038 d9)."""
    path = plugin_dir / HOOKS
    shown = path.relative_to(ROOT)
    if not path.is_file():
        errors.append(f"{shown} is missing: the oml plugin's hooks (ADR-0029 d7, ADR-0038 d9)")
        return
    try:
        hooks = json.loads(path.read_text())["hooks"]
        declared = sorted(hooks)
        tables = {event: hooks.get(event, []) for event in HOOK_EVENTS}
        for groups in tables.values():
            for group in groups:
                list(group["hooks"])
    except (json.JSONDecodeError, KeyError, TypeError, AttributeError) as e:
        errors.append(f"{shown} does not parse as a hooks file ({type(e).__name__}: {e})")
        return
    if declared != sorted(HOOK_EVENTS):
        errors.append(f"{shown} declares the events {declared}, not exactly {sorted(HOOK_EVENTS)}")
    for event, expected in HOOK_EVENTS.items():
        groups = tables[event]
        if len(groups) != len(expected):
            errors.append(f"{shown}: {event} holds {len(groups)} groups, not {len(expected)}")
            continue
        for group, (matcher, timeout, status, args, kind) in zip(groups, expected):
            check_hook(shown, event, [group], matcher, timeout, status, args, kind, errors)


def check_hook(
    shown: Path,
    event: str,
    groups: list,
    matcher: str | None,
    timeout: int,
    status: str | None,
    args: list[str],
    kind: str,
    errors: list[str],
) -> None:
    entries = [h for group in groups for h in group["hooks"]]
    if len(groups) != 1 or len(entries) != 1 or entries[0].get("type") != "command":
        types = [h.get("type") for h in entries]
        errors.append(f"{shown}: {event} holds the hooks {types}, not exactly one command in one group")
        return
    entry = entries[0]
    if groups[0].get("matcher") != matcher:
        errors.append(f"{shown}: the {event} matcher is {groups[0].get('matcher')!r}, not {matcher!r}")
    if entry.get("timeout") != timeout:
        errors.append(f"{shown}: the {event} timeout is {entry.get('timeout')!r}, not {timeout}")
    if entry.get("statusMessage") != status:
        errors.append(f"{shown}: the {event} statusMessage is {entry.get('statusMessage')!r}, not {status!r}")
    command = entry.get("command", "")
    if kind == "checked":
        if entry.get("onFailure") != "block":
            errors.append(f"{shown}: the {event} onFailure is {entry.get('onFailure')!r}, not 'block'")
        check_checked_command(shown, event, command, args, errors)
        return
    if "onFailure" in entry:
        errors.append(f"{shown}: the {event} {' '.join(args)} hook sets onFailure, which only the guardrail check takes")

    with tempfile.TemporaryDirectory() as tmp:
        no_oml = Path(tmp) / "no-oml"
        no_oml.mkdir()
        run = sh(command, no_oml)
        if (run.returncode, run.stdout, run.stderr) != (0, "", ""):
            errors.append(
                f"{shown}: with no oml on PATH the {event} command exits {run.returncode} "
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
        if ran != args:
            errors.append(f"{shown}: the {event} command runs oml with {ran}, not `{' '.join(args)}`: {command}")
        if run.returncode != 0:
            errors.append(f"{shown}: a failing oml makes the {event} command exit {run.returncode}, not 0: {command}")


def check_checked_command(shown: Path, event: str, command: str, args: list[str], errors: list[str]) -> None:
    """The guardrail check's command: with no `oml`, or one older than the
    verb (exit 2 on an unknown subcommand, so its `--help` probe fails), it
    exits 0 in silence; with an `oml` whose verb fails it exits non-zero, so
    the call is blocked, never allowed by a crash (proposed ADR-0037 d9)."""
    with tempfile.TemporaryDirectory() as tmp:
        no_oml = Path(tmp) / "no-oml"
        no_oml.mkdir()
        run = sh(command, no_oml)
        if (run.returncode, run.stdout, run.stderr) != (0, "", ""):
            errors.append(f"{shown}: with no oml on PATH the {event} command exits {run.returncode}, not 0 in silence: {command}")

        older = Path(tmp) / "older"
        older.mkdir()
        (older / "oml").write_text("#!/bin/sh\nexit 2\n")
        (older / "oml").chmod(0o755)
        run = sh(command, older)
        if (run.returncode, run.stdout, run.stderr) != (0, "", ""):
            errors.append(f"{shown}: with an oml older than the verb the {event} command exits {run.returncode}, not 0 in silence: {command}")

        failing = Path(tmp) / "failing"
        failing.mkdir()
        argv = Path(tmp) / "argv"
        (failing / "oml").write_text(
            "#!/bin/sh\n"
            "for a in \"$@\"; do [ \"$a\" = --help ] && exit 0; done\n"
            f"printf '%s\\n' \"$@\" > '{argv}'\nexit 1\n"
        )
        (failing / "oml").chmod(0o755)
        run = sh(command, failing)
        ran = argv.read_text().splitlines() if argv.exists() else None
        if ran != args:
            errors.append(f"{shown}: the {event} command runs oml with {ran}, not `{' '.join(args)}`: {command}")
        if run.returncode == 0:
            errors.append(f"{shown}: an oml whose verb fails makes the {event} command exit 0, so a crash would allow the call: {command}")


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
            "plugin check: the oml plugin agrees with the server, its hooks are guarded, "
            "and oml-admin issues keys only with --out"
        )
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
