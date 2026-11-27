//! `oml` — every administrative ceremony is a CLI verb riding the lab's own
//! HTTP API (ADR-0015, cli/reference.md). Exit codes per the reference's
//! conventions: 0 success, 1 a typed server refusal (or a failed
//! verification/doctor verdict), 2 local failure. `--json` emits exactly one
//! JSON document on stdout — the server's typed response, unreshaped. The one
//! exception is `oml hook session-start`, which always exits 0 and prints the
//! hook document Claude Code reads, or nothing (ADR-0029).

// The argument doc comments are clap's --help text, where a placeholder such
// as <role> must print as written rather than as markdown code.
#![allow(rustdoc::invalid_html_tags)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{ArgGroup, Args, Parser, Subcommand};
use serde_json::{Value, json};

use oml_cli::client::{CallOpts, CliError, Client, encode, new_idempotency_key, segment};
use oml_cli::creds::{self, Credentials};
use oml_cli::discovery;
use oml_cli::hook;
use oml_cli::issue::{
    KeyFile, configure_line, connect_code_printed, key_limit_lines, minted_code_printed, parse_ttl,
    setup_line, sh_quote, with_reader_beside_submitters,
};
use oml_cli::verify::parse_verifying_key;

#[derive(Parser)]
#[command(
    name = "oml",
    version,
    about = "Administer Open Memory Lab through its HTTP API."
)]
struct Cli {
    /// Server URL without /mcp. URL and API key must come together from flags,
    /// environment, or credentials file, in that order.
    #[arg(long, global = true, env = "OML_URL")]
    url: Option<String>,
    /// API key paired with --url. Prefer OML_API_KEY or the credentials file
    /// because argv is visible to other processes.
    #[arg(
        long = "api-key",
        global = true,
        env = "OML_API_KEY",
        hide_env_values = true
    )]
    api_key: Option<String>,
    /// Emit the server's typed response as one JSON document on stdout.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
    /// Each start setting by the source that set it, filled from the parsed
    /// matches, which alone tell a flag from the environment.
    #[arg(skip)]
    start: Start,
}

/// The start settings each source set, an empty value setting nothing
/// (ADR-0039 d15).
#[derive(Debug, Default)]
struct Start {
    flag_url: Option<String>,
    flag_key: Option<String>,
    env_url: Option<String>,
    env_key: Option<String>,
}

impl Start {
    /// Splits `cli`'s `--url` and `--api-key` by where clap took each from.
    fn of(cli: &Cli, matches: &clap::ArgMatches) -> Self {
        use clap::parser::ValueSource;
        let set = |v: &Option<String>| v.clone().filter(|s| !s.is_empty());
        let from = |id: &str, source: ValueSource| matches.value_source(id) == Some(source);
        let pick = |id: &str, v: &Option<String>, source: ValueSource| {
            if from(id, source) { set(v) } else { None }
        };
        Self {
            flag_url: pick("url", &cli.url, ValueSource::CommandLine),
            flag_key: pick("api_key", &cli.api_key, ValueSource::CommandLine),
            env_url: pick("url", &cli.url, ValueSource::EnvVariable),
            env_key: pick("api_key", &cli.api_key, ValueSource::EnvVariable),
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Exchange an enrollment token for the first admin key.
    Bootstrap {
        #[command(subcommand)]
        command: BootstrapCommand,
    },
    /// Manage tenants (operator tier).
    Tenants {
        #[command(subcommand)]
        command: TenantsCommand,
    },
    /// Show or export guardrails in force for this key.
    Guardrails {
        #[command(subcommand)]
        command: GuardrailsCommand,
    },
    /// List the skills offered at this key's scope.
    Skills {
        #[command(subcommand)]
        command: SkillsCommand,
    },
    /// Manage keys and connections.
    Keys {
        #[command(subcommand)]
        command: KeysCommand,
    },
    /// Manage global per-key denials, which override all grants.
    Deny {
        #[command(subcommand)]
        command: DenyCommand,
    },
    /// Manage a person's chat memory.
    Chat {
        #[command(subcommand)]
        command: ChatCommand,
    },
    /// Manage workspaces.
    Workspaces {
        #[command(subcommand)]
        command: WorkspacesCommand,
    },
    /// Manage claim links and workspace brief entries, proposed and ratified by different keys.
    Edges {
        #[command(subcommand)]
        command: EdgesCommand,
    },
    /// Manage named crew records (admin key).
    Crews {
        #[command(subcommand)]
        command: CrewsCommand,
    },
    /// Manage the email allowlist for web sign-in (admin key).
    Web {
        #[command(subcommand)]
        command: WebCommand,
    },
    /// Show or set search pipelines (admin key).
    Search {
        #[command(subcommand)]
        command: SearchCommand,
    },
    /// Manage recall telemetry and replay real usage (admin key).
    Usage {
        #[command(subcommand)]
        command: UsageCommand,
    },
    /// Read or rescan memories; admin reads are recorded in history (admin key).
    Admin {
        #[command(subcommand)]
        command: AdminCommand,
    },
    /// Submit a claim for review, replace one with --replaces, or propose its retirement.
    Write(WriteArgs),
    /// Fetch a memory by artifact id, recall with --query, or list memories.
    Read(ReadArgs),
    /// Withdraw a held version this key wrote, for a mistaken write; it is kept
    /// as withdrawn and never served.
    Withdraw(WithdrawArgs),
    /// Tail journal event digests within the key's reader access.
    Events(EventsArgs),
    /// Sync a workspace with committed repository documents: extract, diff, write,
    /// check, or link claims.
    Sync(SyncArgs),
    /// Print a JSON source anchor for a committed file or fragment at --rev, for
    /// oml write --anchor.
    Anchor(AnchorArgs),
    /// Review held submissions.
    Review {
        #[command(subcommand)]
        command: ReviewCommand,
    },
    /// Verify audit evidence.
    Audit {
        #[command(subcommand)]
        command: AuditCommand,
    },
    /// Read an API key without echo from the terminal or stdin, verify it, and
    /// store the URL and key owner-only.
    Login {
        /// Server URL without /mcp.
        // Not `url`: clap would merge that id with the global --url.
        #[arg(value_name = "URL")]
        target: String,
    },
    /// Check health signals; exit 1 if any fail.
    Doctor,
    /// Start OML locally with Docker, issue agent and reviewer keys, and verify a write.
    Quickstart(QuickstartArgs),
    /// Install and configure an agent tool's OML plugin.
    Setup {
        #[command(subcommand)]
        command: SetupCommand,
    },
    /// Run Claude Code plugin hooks under the agent's key.
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
    /// Write demonstration data through the public API.
    Demo {
        #[command(subcommand)]
        command: DemoCommand,
    },
}

#[derive(Subcommand)]
enum DemoCommand {
    /// Create 30 fictional-team memories (20 approved, 4 superseded, 6 held),
    /// then fence their author and reviewer identities. Admin key; no keys are printed.
    Seed {
        /// New workspace name; it must not exist.
        #[arg(value_name = "WORKSPACE")]
        workspace: String,
        /// Parent workspace.
        #[arg(long, default_value = "universe")]
        parent: String,
    },
}

#[derive(Args)]
struct QuickstartArgs {
    /// Directory for compose files, credentials, and keys; refuses a previous quickstart.
    #[arg(long, default_value = "oml")]
    dir: PathBuf,
    /// Port on 127.0.0.1 for the lead.
    #[arg(long, default_value_t = 8080)]
    port: u16,
    /// The person the quickstart's admin key belongs to, by user name.
    #[arg(long)]
    person: String,
    /// That person's sign-in email for the console.
    #[arg(long)]
    email: String,
}

#[derive(Subcommand)]
enum SetupCommand {
    /// Install the matching oml plugin through the claude CLI, passing the URL and key
    /// on stdin. Admin and operator keys are refused. Print commands with --print or
    /// when claude is unavailable. A key without --url asks for the server on a terminal
    /// and is refused elsewhere.
    ClaudeCode(SetupClaudeCodeArgs),
}

#[derive(Args)]
struct SetupClaudeCodeArgs {
    /// Key file from `oml keys issue-agent --out`, used instead of --api-key and
    /// paired with --url alone, never with OML_URL.
    #[arg(long)]
    key_file: Option<PathBuf>,
    /// Print claude commands without running them; configure reads the key file or OML_API_KEY.
    #[arg(long)]
    print: bool,
}

#[derive(Subcommand)]
enum GuardrailsCommand {
    /// Print all guardrails as the hook frames them; exit 1 if there are more than twelve.
    Show {
        /// Scope workspace by id or name; omitted, the key's default.
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Export promoted guardrail text for a harness without plugin hooks.
    /// claude-md writes a whole .claude/rules/ file; agents-md updates text
    /// between <!-- oml:guardrails --> markers.
    Export {
        /// agents-md | claude-md
        #[arg(long, value_parser = ["agents-md", "claude-md"])]
        format: String,
        /// File to write.
        #[arg(long)]
        out: PathBuf,
        /// Write nothing; exit 1 if the file differs from the live set.
        #[arg(long)]
        check: bool,
        /// Scope workspace by id or name; omitted, the key's default.
        #[arg(long)]
        workspace: Option<String>,
    },
}

#[derive(Subcommand)]
enum SkillsCommand {
    /// Print every offered skill: command, version, source workspace and description.
    List {
        /// Scope workspace by id or name; omitted, the key's default.
        #[arg(long)]
        workspace: Option<String>,
        /// Also list this key's own held skills, marked unreviewed.
        #[arg(long)]
        include_own_held: bool,
    },
}

#[derive(Subcommand)]
enum ChatCommand {
    /// Open the key's `chat` workspace under its user root and print OML_CHAT_WORKSPACE
    /// for plugin capture and recall. Requires a person's key.
    Init {
        /// Parent by id or name; omitted, the key's user root.
        #[arg(long)]
        parent: Option<String>,
    },
}

#[derive(Subcommand)]
enum HookCommand {
    /// Count and hash the turn for chat capture; with OML_TURN_RECALL=1, inject
    /// recalled memories within budget. Always exits 0.
    UserPrompt,
    /// With OML_CHAT_CAPTURE=1, ask the agent to write facts from uncaptured
    /// turns when capture is due. Always exits 0.
    Stop,
    /// Record session compaction so the next Stop captures immediately; print
    /// nothing and never block.
    PreCompact,
    /// Print SessionStart context from plugin url/api_key options or
    /// OML_URL/OML_API_KEY: steering, recent submissions, and matching memories.
    /// Print nothing on failure; always exit 0.
    SessionStart,
    /// With OML_FAILURE_RECALL=1, print memories matching a failed Bash command's
    /// error using plugin options or OML_URL/OML_API_KEY. Always exits 0.
    ToolFailure,
    /// Delete this session's failure-recall cache; print nothing and always exit 0.
    SessionEnd,
    /// Print guardrails for this agent's key; at SessionStart, write the session
    /// rule cache. Failures are shown to the user and exit 0.
    Guardrails,
    /// Check a tool call against the session rule cache without network access;
    /// return deny, ask, or nothing, never allow. Missing cache warns and decides
    /// nothing; failures exit 2 and block the call.
    PreToolUse,
}

#[derive(Subcommand)]
enum BootstrapCommand {
    /// Exchange a single-use enrollment token for the first admin key and store
    /// the URL and key owner-only (0600).
    Claim {
        /// Name for the minted key; unique in its namespace and never released.
        key_name: String,
        /// Read the enrollment token from stdin without terminal echo, keeping it
        /// out of argv and shell history; required.
        #[arg(long, required = true)]
        token_stdin: bool,
        /// Name for the tenant to create.
        #[arg(long)]
        tenant_name: Option<String>,
        /// Principal for the first admin key.
        #[arg(long)]
        principal: Option<String>,
        /// The person the first admin key belongs to: the claim creates that
        /// person's user root and links the key to it. Required, since every
        /// admin key belongs to a person.
        #[arg(long)]
        person: String,
        /// The person's sign-in email, recorded as accepted for the console:
        /// the one way the person --person names gets a first sign-in.
        /// Required.
        #[arg(long)]
        email: String,
    },
}

#[derive(Subcommand)]
enum TenantsCommand {
    /// Create a tenant (not available in the standalone PoC).
    Create { name: String },
}

#[derive(Subcommand)]
enum KeysCommand {
    /// Issue a key and print its value once.
    Issue(KeysIssueArgs),
    /// Issue an agent-plugin key and print OML_URL/OML_API_KEY exports; with
    /// --out, print the claude configure line that reads it.
    IssueAgent(IssueAgentArgs),
    /// Show a key's grants, denials, crew, and epoch, including revoked keys;
    /// never print its value.
    Show {
        /// Key id or name.
        // Not `key`: clap would merge that id with the global --key.
        #[arg(value_name = "KEY")]
        key_ref: String,
    },
    /// Mint a connector key's connect code, void earlier codes, and print the
    /// connector URL and code.
    ConnectCode {
        /// Connector key id or name.
        #[arg(value_name = "KEY")]
        key_ref: String,
        /// Code lifetime, such as 90s, 45m, 12h, or 7d, up to 30d; defaults to
        /// 10m. Longer than 10m requires a key granted on exactly one non-universe
        /// workspace.
        #[arg(long, value_name = "DURATION")]
        ttl: Option<String>,
    },
    /// Set or clear a key's expiry and its requests per minute; needs an admin
    /// key other than the key itself, and journals key.limits_set. An admin key
    /// takes neither limit.
    Limit {
        /// Key id or name.
        #[arg(value_name = "KEY")]
        key_ref: String,
        /// Instant the key stops authenticating, RFC 3339 with its offset, such
        /// as 2026-12-01T17:00:00Z; must be later than now.
        #[arg(long, value_name = "INSTANT", conflicts_with = "no_expiry")]
        expires: Option<String>,
        /// Clear the expiry, so the key does not expire.
        #[arg(long)]
        no_expiry: bool,
        /// Requests the key may make in one UTC minute, from 1 to 1000000.
        #[arg(long, value_name = "N", conflicts_with = "no_rate_limit")]
        requests_per_minute: Option<u32>,
        /// Clear the key's own rate, leaving any quota of its account.
        #[arg(long)]
        no_rate_limit: bool,
    },
    /// Give a key a new value and print it once, or with --out write it to a
    /// new owner-only file and print none. The key keeps its name, grants,
    /// denials and limits; journals key.rotated. A key rotates itself; an
    /// admin key rotates its own admin key and a tenant key that names no
    /// person.
    Rotate {
        /// Key id or name.
        #[arg(value_name = "KEY")]
        key_ref: String,
        /// How long the previous value keeps working, such as 1h or 24h, at
        /// most 24h; omitted, it stops at once.
        #[arg(long, value_name = "DURATION")]
        grace: Option<String>,
        /// Write the new value to this new file, owner-only (0600), and print
        /// no value.
        #[arg(long, value_name = "FILE")]
        out: Option<std::path::PathBuf>,
    },
    /// Revoke one key: it authenticates nothing afterwards on any path, and
    /// the keys it issued keep working. Journals key.revoked. An admin key
    /// revokes any key of the tenant except a session key.
    Revoke {
        /// Key id or name.
        #[arg(value_name = "KEY")]
        key_ref: String,
        /// Why the key is revoked; scanned and journalled.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Link the calling admin key to a person, pending until that person
    /// accepts the link on the console's Keys page; journals
    /// key.linked_person. The one link a key makes to its own holder.
    LinkSelf {
        /// The person's user name.
        #[arg(long)]
        person: String,
    },
    /// Revoke one connector-key connection and journal connection.revoked.
    Disconnect {
        /// Connector key id or name.
        #[arg(value_name = "KEY")]
        key_ref: String,
        /// Connection id listed by `oml keys show`.
        connection_id: String,
        /// Reason for revocation; only its digest is journalled.
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Args)]
struct KeysIssueArgs {
    /// Key name, unique in the tenant and never released; distinct from principal and label.
    key_name: String,
    /// reader | contributor | submitter | reviewer | admin. A contributor
    /// recalls and writes; a submitter writes without recall.
    #[arg(long)]
    role: String,
    /// agent | person | service
    #[arg(long, default_value = "agent")]
    used_by: String,
    /// Default workspace (id or name) for provider requests without one; must be
    /// covered by a grant. Defaults to the first grant's workspace.
    #[arg(long)]
    default_workspace: Option<String>,
    /// Grant <workspace>:<role>, or <workspace>:<role>:node for one node; workspace
    /// may be id or name. Repeatable. For a contributor key, each submitter grant
    /// brings a reader grant on the same workspace. A create suffix,
    /// <workspace>:<role>:create, lets the key create child workspaces there;
    /// granting it waits on a second admin person.
    #[arg(long = "grant", value_parser = parse_grant)]
    grants: Vec<(String, String)>,
    /// Create the workspace <parent>/<name> in the same act and grant the key its
    /// role's default grants on it, covering the subtree. Repeatable. Nothing is
    /// created while the issuance waits on a second admin person.
    #[arg(long = "create-workspace", value_name = "PARENT/NAME", value_parser = parse_new_workspace)]
    create_workspaces: Vec<(String, String)>,
    /// Crew record (id or name), fixed for the key's life.
    #[arg(long)]
    crew: Option<String>,
    /// User the key acts for. Grants must be under `universe.<user>`, where
    /// workspace names resolve; the first key creates that root.
    #[arg(long)]
    user: Option<String>,
    /// The person an admin key belongs to, by user name; required with
    /// --role admin and refused on any other role. It implies --used-by
    /// person, and the key authenticates nothing until that person accepts
    /// its link on the console's Keys page.
    #[arg(long)]
    person: Option<String>,
    /// Principal the key acts as.
    #[arg(long)]
    principal: Option<String>,
    #[arg(long)]
    label: Option<String>,
    /// Guardrail scope: workspace loads only the default workspace's guardrails;
    /// ancestors also loads every readable ancestor. Defaults to ancestors.
    #[arg(long, value_parser = ["workspace", "ancestors"])]
    guardrail_scope: Option<String>,
    /// Key profile: curator is an agent-held reviewer that narrows within an
    /// in-progress workspace and never promotes.
    #[arg(long, value_parser = ["curator"])]
    profile: Option<String>,
    /// Why the key is issued; scanned and journalled with the issuance. An
    /// admin key's issuance waits on a second admin person and requires it,
    /// except in a tenant that has only ever had one admin person.
    #[arg(long)]
    reason: Option<String>,
    /// Write the value to a new owner-only (0600) file, refusing one that
    /// exists, and print its path instead of the value; with --json the
    /// answer names the path as key_file and carries no value.
    #[arg(long)]
    out: Option<PathBuf>,
    #[command(flatten)]
    limits: KeyLimitArgs,
}

/// The limits an issuing verb may set on the key it issues.
#[derive(Args)]
struct KeyLimitArgs {
    /// Instant the key stops authenticating, RFC 3339 with its offset, such as
    /// 2026-12-01T17:00:00Z; must be later than now. Omitted, the key does not
    /// expire. Refused for an admin key.
    #[arg(long, value_name = "INSTANT")]
    expires: Option<String>,
    /// Requests the key may make in one UTC minute, from 1 to 1000000. Omitted,
    /// the key has no rate of its own. Refused for an admin key.
    #[arg(long, value_name = "N")]
    requests_per_minute: Option<u32>,
}

impl KeyLimitArgs {
    /// Adds the limits the verb was given to an issuance body.
    fn add_to(&self, body: &mut Value) {
        if let Some(expires) = &self.expires {
            body["expiry"] = json!(expires);
        }
        if let Some(rate) = self.requests_per_minute {
            body["requests_per_minute"] = json!(rate);
        }
    }
}

#[derive(Args)]
struct IssueAgentArgs {
    /// Key name, unique in the tenant and never released; not derived from the principal.
    key_name: String,
    /// Principal the key acts as; give each agent its own.
    #[arg(long)]
    principal: String,
    /// Workspace (id or name) to write and recall, and its default. With --user,
    /// optional and resolved in that user's namespace; defaults to the user's root.
    #[arg(long)]
    workspace: Option<String>,
    /// User the key acts for, under `universe.<user>`; the first key for a user
    /// creates that root.
    #[arg(long)]
    user: Option<String>,
    /// Crew record (id or name), fixed for the key's life.
    #[arg(long)]
    crew: Option<String>,
    /// Who holds the key.
    #[arg(long, default_value = "agent", value_parser = ["agent", "person"])]
    used_by: String,
    /// contributor recalls and writes the workspace; submitter writes it without
    /// recall; reader only recalls it.
    #[arg(long, default_value = "contributor", value_parser = ["contributor", "submitter", "reader"])]
    role: String,
    /// Write the key to a new 0600 file without overwriting and print a line that
    /// reads it instead of the value.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Issue a claude.ai custom-connector key with a single-use connect code
    /// instead of a key value.
    #[arg(long)]
    connect_code: bool,
    /// Guardrail scope: workspace loads only the default workspace's guardrails;
    /// ancestors also loads every readable ancestor. Defaults to ancestors.
    #[arg(long, value_parser = ["workspace", "ancestors"])]
    guardrail_scope: Option<String>,
    /// Create the workspace <parent>/<name> in the same act and grant the key its
    /// role's default grants on it, covering the subtree. Repeatable. Nothing is
    /// created while the issuance waits on a second admin person.
    #[arg(long = "create-workspace", value_name = "PARENT/NAME", value_parser = parse_new_workspace)]
    create_workspaces: Vec<(String, String)>,
    #[command(flatten)]
    limits: KeyLimitArgs,
}

#[derive(Subcommand)]
enum DenyCommand {
    /// Create or extend a global deny over a subtree, overriding all grants;
    /// journals key.denied.
    Set {
        /// Key to deny, by id or name.
        // Not `key`: clap would merge that id with the global --key.
        #[arg(value_name = "KEY")]
        key_ref: String,
        /// Workspace (id or name) whose subtree the key can no longer access.
        subtree: String,
        /// Reason for denial; accepted by the server but not journalled.
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand)]
enum WorkspacesCommand {
    /// Create a child workspace in development status.
    Create {
        /// Parent workspace, by id or name.
        parent: String,
        /// New workspace name, unique in the tenant and immutable.
        name: String,
        /// Crew record to assign, by id or name.
        #[arg(long)]
        crew: Option<String>,
    },
    /// List a workspace tree with each node's path, id, status, and crew.
    Tree {
        /// Root workspace, by id or name.
        #[arg(value_name = "ROOT", conflicts_with = "root")]
        positional_root: Option<String>,
        /// Root workspace, by id or name.
        #[arg(long, conflicts_with = "positional_root")]
        root: Option<String>,
    },
    /// Show a workspace's path, status, crew, and children.
    Show {
        /// Workspace id or name.
        workspace: String,
    },
    /// Set who sees the drafts: in_progress makes them visible to everyone
    /// working here, labelled unreviewed, and standard to the writing key alone,
    /// which is refused while items are held. Reviewer keys used by a person only.
    Mode {
        /// Workspace id or name.
        workspace: String,
        /// `in_progress` (drafts visible to everyone working here) or `standard`
        /// (drafts visible to the writing key).
        #[arg(value_parser = ["in_progress", "standard"])]
        mode: String,
    },
    /// Set whether recalls scoped elsewhere may search this workspace when
    /// they ask for wider sources; grants nothing. Person-held reviewer keys or
    /// an admin key; journals workspace.federation_set.
    Federation {
        /// Workspace id or name.
        workspace: String,
        /// `on` lets federated recalls search it (the default); `off` declines.
        #[arg(value_parser = ["on", "off"])]
        accept: String,
        /// Why: an admin's `on` waits on another admin person and requires
        /// one, unless the tenant has only ever had one admin person.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Mark a protected or frozen workspace public, so every reading key of
    /// the tenant reads its approved items and recalls them by default, or
    /// clear the mark. An admin person only; a set waits on another admin
    /// person unless the tenant has only ever had one.
    Public {
        #[command(subcommand)]
        command: WorkspacePublicCommand,
    },
    /// Set a workspace's crew record (admin key).
    Crew {
        #[command(subcommand)]
        command: WorkspaceCrewCommand,
    },
    /// Export a workspace's subtree as a signed bundle encrypted to named age
    /// recipients, written to a new owner-only file. A person's key only;
    /// journals workspace.exported.
    Export {
        /// Workspace id or name.
        workspace: String,
        /// An age X25519 public key (`age1…`) to encrypt to; repeat for more.
        /// At least one: a bundle is never written unencrypted.
        #[arg(long = "recipient", value_name = "AGE")]
        recipients: Vec<String>,
        /// The new file to write; an existing file is never overwritten.
        #[arg(long)]
        out: PathBuf,
        /// History mode: every version ever promoted and the key's own drafts.
        /// The default in a person's own root.
        #[arg(long, conflicts_with_all = ["release", "since"])]
        history: bool,
        /// Release mode, one published release (refused until releases are built).
        #[arg(long)]
        release: Option<String>,
        /// The difference since a base release (refused until releases are built).
        #[arg(long)]
        since: Option<String>,
    },
    /// Move through development, protected and frozen; each step needs
    /// proposal and ratification by different keys.
    Status {
        #[command(subcommand)]
        command: WorkspaceStatusCommand,
    },
    /// Import an encrypted bundle into an in-progress workspace, or a new
    /// name created in progress. The bundle is decrypted here, twice, and
    /// verified before upload; nothing decrypted is written to disk. Every
    /// version lands held by this key. A contributor key used by a person only.
    Import {
        /// The encrypted bundle (`.oml-bundle`).
        file: std::path::PathBuf,
        /// Target workspace, by id or name; an absent name is created.
        #[arg(long)]
        into: String,
        /// A file holding the age identity the bundle is encrypted to.
        #[arg(long)]
        identity: std::path::PathBuf,
        /// The source key's fingerprint, checked out of band; required on
        /// the first import from a source, which pins it. A target name
        /// that does not exist yet is refused without it before anything is
        /// uploaded; for an existing target only the server knows whether it
        /// pins this source, so the import request answers.
        #[arg(long)]
        expect_key: Option<String>,
        /// The source tenant id the bundle must name.
        #[arg(long)]
        expect_tenant: Option<String>,
        /// Print the plan and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Admit a bundle older than the last one imported from its source.
        #[arg(long)]
        allow_older: bool,
        /// A completed upload of this bundle, as a dry run names it, imported
        /// without sending the bundle again.
        #[arg(long)]
        upload: Option<String>,
    },
}

#[derive(Subcommand)]
enum WorkspaceStatusCommand {
    /// Propose the next forward rung; it waits for a second identity.
    Propose {
        /// The workspace, by id or name.
        workspace: String,
        /// The rung proposed: protected or frozen.
        #[arg(long)]
        to: String,
    },
    /// Ratify a pending proposal under a key that did not propose it.
    Ratify {
        /// The workspace, by id or name.
        workspace: String,
        /// The proposal's transition id.
        transition_id: String,
    },
    /// The workspace's live proposals and the review policy in force.
    Pending {
        /// The workspace, by id or name.
        workspace: String,
    },
}

#[derive(Subcommand)]
enum EdgesCommand {
    /// Propose an edge for ratification by another key.
    Propose {
        #[command(subcommand)]
        kind: EdgeProposeCommand,
    },
    /// Ratify a pending edge under a key that did not propose it.
    Ratify {
        /// Edge id.
        edge_id: String,
    },
    /// List one page of pending and ratified edges, newest first.
    List {
        /// claim or brief.
        #[arg(long)]
        kind: String,
        /// For claim, filter by subtree; for brief, required workspace.
        #[arg(long)]
        workspace: Option<String>,
        /// For claim, only edges touching this artifact.
        #[arg(long)]
        artifact: Option<String>,
        /// Page size.
        #[arg(long)]
        limit: Option<u32>,
        /// Previous page's next_cursor.
        #[arg(long)]
        cursor: Option<String>,
    },
}

#[derive(Subcommand)]
enum EdgeProposeCommand {
    /// Link claims with a label such as supports, contradicts, relates_to, or continues.
    Claim {
        /// Source claim artifact id.
        from: String,
        /// Target claim artifact id.
        to: String,
        #[arg(long)]
        label: String,
    },
    /// Propose a promoted claim for a workspace's brief.
    Brief {
        /// Claim artifact id.
        artifact: String,
        /// Workspace id or name.
        workspace: String,
    },
}

#[derive(Subcommand)]
enum WorkspaceCrewCommand {
    /// Set the workspace's crew record, including on frozen workspaces;
    /// journals workspace.crew_updated.
    Set {
        /// Workspace id or name.
        workspace: String,
        /// Crew record id or name.
        #[arg(long)]
        crew: String,
    },
}

#[derive(Subcommand)]
enum AdminCommand {
    /// Read one memory with text and attribution, or search all organisation
    /// memories with --query, --workspace, or --state; journals memory.admin_read.
    Read {
        /// Memory artifact id; omit to search.
        memory: Option<String>,
        /// Full-text search using recall matching.
        #[arg(long)]
        query: Option<String>,
        /// Limit to this workspace subtree (id, name, or qualified name).
        #[arg(long)]
        workspace: Option<String>,
        /// held, promoted, rejected, superseded, demoted or withdrawn; an
        /// erased version is listed as demoted.
        #[arg(long)]
        state: Option<String>,
        /// The open investigation the read is made under (`oml admin
        /// investigate --reason`); every read of claim text names one.
        #[arg(long)]
        investigation: String,
    },
    /// Open an investigation with the reason every admin read made under it
    /// carries, print its id, or with --close end one; with neither, list the
    /// key's open investigations. Journals admin.investigation_opened or
    /// admin.investigation_closed.
    Investigate {
        /// Why the text is read; stored in clear and scanned.
        #[arg(long, conflicts_with = "close")]
        reason: Option<String>,
        /// The id of an open investigation of this key to close.
        #[arg(long)]
        close: Option<String>,
    },
    /// Take a marked break-glass fix that serves at once and waits for a
    /// person to confirm or revert it: demote an approved version, edit or
    /// retire the served tip, or erase the claim. Journals admin.fix with
    /// the lifecycle event it causes; an admin key used by a person only.
    Fix {
        /// demote, edit, retire or erase.
        #[arg(value_parser = ["demote", "edit", "retire", "erase"])]
        act: String,
        /// The version's artifact id.
        target: String,
        /// Why; stored in clear and scanned.
        #[arg(long)]
        reason: String,
        /// An edit's new body; `-` reads it from stdin.
        #[arg(long)]
        body: Option<String>,
        /// An edit's new title, where its format takes one.
        #[arg(long)]
        title: Option<String>,
    },
    /// List the tenant's admin acts that wait on a second admin person, or
    /// with --state another state, or all.
    Acts {
        /// all, effective, pending_confirmation, confirmed, declined, expired,
        /// contested or settled; defaults to pending_confirmation.
        #[arg(long)]
        state: Option<String>,
    },
    /// Confirm another admin person's pending act as the second admin
    /// person; it takes effect now. Journals admin.act.
    Confirm {
        /// The act's id, listed by `oml admin acts`.
        act: String,
    },
    /// Decline a pending admin act, which then never takes effect, or undo
    /// a widening act taken at once in a tenant with one admin person: the
    /// key, grant, code, limit or admin flag it applied. Journals admin.act.
    Decline {
        /// The act's id, listed by `oml admin acts`.
        act: String,
        /// Why; scanned and journalled.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Settle an open act against another admin person as a third admin
    /// person: --confirm revokes the target's suspended admin keys, --reverse
    /// lifts the suspension and restores the entry. Below three admin
    /// persons the operator settles from the host. Journals admin.act.
    Settle {
        /// The act's id, listed by `oml admin acts --state effective`.
        act: String,
        /// Confirm the act.
        #[arg(long, conflicts_with = "reverse", required_unless_present = "reverse")]
        confirm: bool,
        /// Reverse the act.
        #[arg(long)]
        reverse: bool,
    },
    /// List the tenant's admin fixes no person has confirmed or reverted,
    /// or with --all every fix.
    Fixes {
        /// Every fix, confirmed and reverted ones included.
        #[arg(long)]
        all: bool,
    },
    /// Report by rule and location which stored versions current rules would
    /// refuse or hold; never prints text or changes data.
    Rescan {
        /// Also record each held version's findings under current rules, so
        /// a held version written before an upgrade is served inside an
        /// in-progress workspace again when nothing is found; journals
        /// scan.findings_recorded.
        #[arg(long)]
        record_held: bool,
        /// Print only aggregates for the operator, not for publication: per
        /// class, per origin kind and per class within an origin, how many
        /// versions the scan would refuse or hold and their share of those
        /// scanned, counts under 5 as <5; no artifact id, claim key or text.
        #[arg(long, conflicts_with = "record_held")]
        rates: bool,
    },
}

#[derive(Subcommand)]
enum WorkspacePublicCommand {
    /// Mark the workspace public: an admin act of class widening, which
    /// waits on another admin person and journals workspace.public_set when
    /// it takes effect.
    Set {
        /// Workspace id or name.
        workspace: String,
        /// Why every reading key of the tenant should read it.
        #[arg(long)]
        reason: String,
    },
    /// Clear the mark: narrowing, at once; a set that waits is declined.
    Clear {
        /// Workspace id or name.
        workspace: String,
        /// Why the workspace stops being public.
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand)]
enum SearchCommand {
    /// Show served pipelines and the deployment, tenant, and effective recall defaults.
    Show,
    /// Set the tenant's default recall pipeline for subsequent requests;
    /// journals search.default_set.
    SetDefault {
        /// Served pipeline name; `deployment` clears the tenant override.
        pipeline: String,
    },
    /// Show the tenant's federation policy; each option changes it and
    /// journals federation.policy_set. Recall searches beyond its scope only
    /// when enabled and asked (sources wide or a list).
    Federation {
        /// `on` lets recalls ask for wider sources; `off` refuses them.
        #[arg(long, value_parser = ["on", "off"])]
        enabled: Option<String>,
        /// The sources an absent `sources` reads: `scope` or `wide`.
        #[arg(long, value_parser = ["scope", "wide"])]
        default_sources: Option<String>,
        /// The most workspaces a wide recall searches beside its scope, and
        /// the most a sources list names, 0 to 64.
        #[arg(long)]
        max_sources: Option<u32>,
        /// Why: a change that searches more waits on another admin person
        /// and requires one, unless the tenant has only ever had one admin
        /// person.
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand)]
enum UsageCommand {
    /// Show recall recording and query-text settings; --query-text changes and
    /// journals query-text retention (telemetry.settings_set).
    Settings {
        /// `on` keeps future recall query text for replay; `off` stops retention
        /// and clears stored text.
        #[arg(long, value_parser = ["on", "off"])]
        query_text: Option<String>,
    },
    /// List the tenant's recalls from the last 30 days with reader follow-up,
    /// newest first.
    Events {
        /// Maximum events, 1 to 200.
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// Replay labelled recalls through each pipeline under the original keys'
    /// scopes; print only scores.
    Eval {
        /// Comma-separated served pipelines.
        #[arg(long, value_delimiter = ',', required = true)]
        pipelines: Vec<String>,
    },
}

#[derive(Subcommand)]
enum WebCommand {
    /// Add an email or change its user/admin flags; journals web.email_allowed
    /// or web.email_changed and ends its sessions.
    Allow {
        /// Email address, compared whole and ASCII case-insensitively.
        email: String,
        /// Sign-in user, at most 56 characters.
        #[arg(long)]
        user: String,
        /// Allow access to the admin page.
        #[arg(long)]
        admin: bool,
        /// Why: required for a new admin person once the tenant has had two,
        /// and for an act against another admin person.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Remove an email, end its sessions, and journal web.email_removed; the
    /// user's agent keys keep working.
    Remove {
        /// Email address.
        email: String,
        /// Why: required to remove another admin person's entry.
        #[arg(long)]
        reason: Option<String>,
    },
    /// List the tenant's allowed emails.
    List,
}

#[derive(Subcommand)]
enum CrewsCommand {
    /// Create a crew record with an immutable name; journals crew.created.
    Create {
        /// Crew name, unique in the tenant.
        name: String,
        #[arg(long)]
        charter_uri: Option<String>,
        #[arg(long)]
        external_manager: Option<String>,
    },
    /// Update and journal a crew's charter URI or external manager; at least
    /// one flag is required and the name cannot change.
    Update(CrewsUpdateArgs),
    /// List crew records.
    List,
    /// Show one crew record by id or name.
    Show {
        /// Crew id or name.
        crew: String,
    },
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("change")
        .required(true)
        .multiple(true)
        .args(["charter_uri", "external_manager"])
))]
struct CrewsUpdateArgs {
    /// The crew's id or name.
    crew: String,
    #[arg(long)]
    charter_uri: Option<String>,
    #[arg(long)]
    external_manager: Option<String>,
}

#[derive(Args)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per mode of ADR-0057's contract sketch"
)]
struct SyncArgs {
    /// File or directory inside a git clone, read at --rev.
    source: String,
    /// Workspace (id or name) to sync; with --pr, the parent of `<workspace>-pr-<n>`.
    #[arg(long)]
    workspace: String,
    /// Adapter: adr, headings, lines, rust, or delta. Defaults to adr for
    /// decision-record Markdown files and headings otherwise; rust (one claim per
    /// crate and module doc) is never inferred. delta reads Delta Lake tables and
    /// requires the oml-lake build.
    #[arg(long)]
    adapter: Option<String>,
    /// Commit to read; never the working tree.
    #[arg(long, default_value = "HEAD")]
    rev: String,
    /// Repository name for anchors; omitted, derived from the `origin` remote.
    #[arg(long)]
    repo: Option<String>,
    /// Print the plan without sending anything.
    #[arg(long, conflicts_with_all = ["check", "edges"])]
    dry_run: bool,
    /// Report anchored claims and edges against --rev without writing memories.
    #[arg(long)]
    check: bool,
    /// With --check, record source checks.
    #[arg(long, requires = "check")]
    record: bool,
    /// Maximum memories one run writes; each run is one bulk verdict and the
    /// next run continues.
    #[arg(long, visible_alias = "batch", default_value_t = oml_cli::sync::run::MAX_WRITES)]
    max_writes: usize,
    /// Supersede a tip with no anchor in this repository as changed.
    #[arg(long)]
    adopt: bool,
    /// Propose retiring every claim this key wrote under the source, as when a
    /// draft pull request closes.
    #[arg(long, requires = "pr", conflicts_with_all = ["check", "edges"])]
    retire_all: bool,
    /// Propose source-stated edges between promoted tips (reviewer key).
    #[arg(long, conflicts_with = "check")]
    edges: bool,
    /// With --edges, ratify matching pending edges proposed by another key.
    #[arg(long, requires = "edges")]
    ratify: bool,
    /// Allow a server that declares itself shared and record consent for its origin in this clone.
    /// Its admins can read submissions; supersession or retirement does not erase
    /// submitted text or anchors.
    #[arg(long)]
    allow_public_server: bool,
    /// Treat a source matching an older promoted version as a deliberate revert.
    #[arg(long)]
    allow_rollback: bool,
    /// Allow changing the adapter used for this source; retires claims written
    /// by the old adapter.
    #[arg(long, requires = "adapter")]
    change_adapter: bool,
    /// Draft pull request <n>: sync records changed between --base and --rev
    /// into `<workspace>-pr-<n>`.
    #[arg(long)]
    pr: Option<u64>,
    /// Glob of source paths to exclude; repeatable, for example `docs/adr/**`.
    /// Excluded paths are never read, compared, or retired; the set is recorded
    /// for this clone.
    #[arg(long)]
    exclude: Vec<String>,
    /// Allow an exclude set different from the recorded one.
    #[arg(long)]
    change_exclude: bool,
    /// With --pr, the pull request's target branch.
    #[arg(long, default_value = "origin/main", requires = "pr")]
    base: String,
    /// With --adapter delta, canonical source URI (`s3://`, `gs://`, `https://`,
    /// or `file://`) stored in anchors instead of a local mirror or copy path.
    #[arg(long, requires = "adapter")]
    uri_base: Option<String>,
}

#[derive(Args)]
struct AnchorArgs {
    /// Path inside a git clone, optionally with #fragment: decision handle,
    /// heading slug, or line span L<n> or L<n>-L<m>.
    target: String,
    /// Commit to read; never the working tree.
    #[arg(long, default_value = "HEAD")]
    rev: String,
    /// Repository name; omitted, derived from the `origin` remote.
    #[arg(long)]
    repo: Option<String>,
    /// Fragmentation adapter: adr, headings, or lines.
    #[arg(long)]
    adapter: Option<String>,
}

#[derive(Args)]
struct WriteArgs {
    /// Claim text.
    text: String,
    /// Claim key; defaults to `auto-<16 hex>` derived from API key and text.
    /// New versions inherit the target's key.
    #[arg(long, conflicts_with = "replaces")]
    claim_key: Option<String>,
    /// memory | derived_fact | steering | term | term_mapping | vocab_release |
    /// guardrail | skill | document; defaults to memory, or the target's kind
    /// with --replaces.
    #[arg(long)]
    kind: Option<String>,
    /// Skill name, a lowercase label; required with --kind skill, and the
    /// target's name with --replaces.
    #[arg(
        long = "skill-name",
        required_if_eq("kind", "skill"),
        requires = "skill_description"
    )]
    skill_name: Option<String>,
    /// Skill description, one line; required with --kind skill.
    #[arg(
        long = "skill-description",
        required_if_eq("kind", "skill"),
        requires = "skill_name"
    )]
    skill_description: Option<String>,
    /// Guardrail severity: never, must, or should; required for non-retirement guardrails.
    #[arg(long, value_parser = ["never", "must", "should"])]
    severity: Option<String>,
    /// Guardrail oml-gr/1 rule as one JSON object; deny or ask, never allow.
    #[arg(long, value_parser = parse_rule)]
    rule: Option<serde_json::Value>,
    /// Lineage artifact id; repeatable. Journals submissions with lineage as learn.
    #[arg(long = "derives-from")]
    derives_from: Vec<String>,
    /// Workspace (id or name); defaults to the key's default, or the target's
    /// with --replaces, which it may not change.
    #[arg(long)]
    workspace: Option<String>,
    /// Promoted tip artifact id to replace with a new held version.
    #[arg(long, requires = "expected_version")]
    replaces: Option<String>,
    /// Required with --replaces: latest version last read. The server rejects
    /// moved claims with submission.version_conflict.
    #[arg(long = "expected-version", requires = "replaces")]
    expected_version: Option<u32>,
    /// Source anchor as a JSON object or @file: {repo, rev, path, fragment?,
    /// hash?, hasher?} or {uri, rev?, …}.
    #[arg(long, value_parser = parse_anchor_arg)]
    anchor: Option<serde_json::Value>,
    /// Media type: text/plain (default), text/markdown, text/vnd.oml.mermaid,
    /// application/json, application/yaml, or "text/vnd.oml.code; lang=<language>".
    /// If claim text is `-`, read it from stdin.
    #[arg(long)]
    format: Option<String>,
    /// One-line fact shown by the text, at most 100 characters; required for
    /// non-prose formats.
    #[arg(long)]
    title: Option<String>,
    /// Source kind: repo, doc, code, lake, chat, web, or manual; defaults to manual.
    #[arg(long = "origin-kind", value_parser = ORIGIN_KINDS)]
    origin_kind: Option<String>,
    /// Propose retiring the --replaces claim; text explains why.
    #[arg(long, requires = "replaces")]
    retirement: bool,
    /// Reason for the change, journalled with the new version.
    #[arg(long, requires = "replaces")]
    reason: Option<String>,
    /// Idempotency-Key for the write; reusing it replays the first response.
    /// Defaults to a fresh key per run.
    #[arg(long)]
    idempotency_key: Option<String>,
}

#[derive(Args)]
struct ReadArgs {
    /// Fetch one version in any state readable by this key.
    #[arg(value_name = "ARTIFACT_ID")]
    artifact_id: Option<String>,
    /// Recall ranked promoted rows, steering first.
    #[arg(long, conflicts_with = "artifact_id")]
    query: Option<String>,
    /// Number of recall hits, 1 to 100; defaults to 8.
    #[arg(long, requires = "query")]
    k: Option<u32>,
    /// Only these kinds, in recall and in the catalog; repeatable or
    /// comma-separated. Recall without steering among them serves no
    /// steering list.
    #[arg(long, value_delimiter = ',', value_parser = KINDS, conflicts_with = "artifact_id")]
    kinds: Vec<String>,
    /// Include this key's own held rows beside promoted hits.
    #[arg(long, requires = "query")]
    include_unreviewed: bool,
    /// Served search pipeline to run (see `oml search show`); defaults to the
    /// tenant's default.
    #[arg(long, requires = "query")]
    search: Option<String>,
    /// any (default) or all. all requires every query word; if no memory matches
    /// them all, any-word hits are returned and marked fallback.
    #[arg(long = "match", requires = "query")]
    match_mode: Option<String>,
    /// Where to search: scope (default), wide (also the protected workspaces
    /// this key reads elsewhere), or comma-separated workspace names searched
    /// alone. Needs federation on (`oml search federation`).
    #[arg(long, requires = "query")]
    sources: Option<String>,
    /// Subtree (id or name) to read; defaults to the key's workspace, or all
    /// workspaces this key wrote in for --own.
    #[arg(long, conflicts_with = "artifact_id")]
    workspace: Option<String>,
    /// List this key's own rows in every state, with their text, instead
    /// of the catalog.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    own: bool,
    /// Filter by state: held, promoted, demoted, rejected, erased or
    /// withdrawn.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    state: Option<String>,
    /// Only claim keys starting with this prefix, such as skill/ or
    /// doc/runbooks/.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    prefix: Option<String>,
    /// Print each catalog entry's text.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    text: bool,
    /// List versions written at or after this RFC 3339 instant.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    since: Option<String>,
    /// List versions written before this RFC 3339 instant.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    until: Option<String>,
    /// Listing page size.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    limit: Option<u32>,
    /// Previous listing's next_cursor.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    cursor: Option<String>,
    /// Filter by anchor repository, such as github.com/owner/name.
    #[arg(long = "anchor-repo", conflicts_with = "artifact_id")]
    anchor_repo: Option<String>,
    /// Filter by anchor path prefix, such as docs/adr/.
    #[arg(long = "anchor-path-prefix", conflicts_with = "artifact_id")]
    anchor_path_prefix: Option<String>,
    /// Filter by origin kind; repeat for multiple, for example
    /// --origin-kind chat --origin-kind doc.
    #[arg(long = "origin-kind", value_parser = ORIGIN_KINDS, conflicts_with = "artifact_id")]
    origin_kinds: Vec<String>,
}

/// Every claim kind, which `--kinds` takes (ADR-0092 d1, d6).
const KINDS: [&str; 10] = [
    "memory",
    "derived_fact",
    "steering",
    "term",
    "term_mapping",
    "vocab_release",
    "guardrail",
    "skill",
    "document",
    "test",
];

/// The closed set of `origin.kind` (ADR-0063 d1).
const ORIGIN_KINDS: [&str; 7] = ["repo", "doc", "code", "lake", "chat", "web", "manual"];

#[derive(Args)]
struct WithdrawArgs {
    /// The held version's artifact id.
    artifact_id: String,
    /// Why it is withdrawn; journalled with the act.
    #[arg(long)]
    reason: Option<String>,
    /// Idempotency-Key for the withdrawal; reusing it replays the first response.
    /// Defaults to a fresh key per run.
    #[arg(long)]
    idempotency_key: Option<String>,
}

#[derive(Args)]
struct EventsArgs {
    /// Resume after this sequence number, using the previous page's next_after_seq.
    #[arg(long)]
    after_seq: Option<u64>,
    /// Subtree to tail (id or name); defaults to the key's workspace.
    #[arg(long)]
    workspace: Option<String>,
    /// Journal events scanned per call.
    #[arg(long)]
    limit: Option<u32>,
}

#[derive(Subcommand)]
enum ReviewCommand {
    /// Take one held item for this key and serve it.
    Next {
        /// `person`: only steering, guardrails and skills, and a change that
        /// would replace one, which only a person approves. `reviewer`: only
        /// what any reviewer may approve.
        #[arg(long, value_parser = ["person", "reviewer"])]
        needs: Option<String>,
    },
    /// List held items this key may judge, with text, oldest first, without
    /// taking them. Reviewer keys used by a person only.
    List(ReviewListArgs),
    /// Approve the one item this key took (default), or each artifact in --ids.
    Approve {
        /// serve_id printed by `oml review next`.
        #[arg(conflicts_with = "ids")]
        serve_id: Option<String>,
        /// Note journalled with the verdict.
        #[arg(long)]
        note: Option<String>,
        /// Artifact ids as a one-per-line file (`oml review list --ids-only`
        /// output), `-` for stdin, or comma-separated ids.
        #[arg(long)]
        ids: Option<String>,
    },
    /// Reject the claimed serve or each --ids artifact with one shared reason.
    Reject {
        /// serve_id printed by `oml review next`.
        #[arg(conflicts_with = "ids")]
        serve_id: Option<String>,
        /// Reason for rejection; required.
        #[arg(long)]
        reason: String,
        /// Artifact ids, as for approve.
        #[arg(long)]
        ids: Option<String>,
    },
    /// List the unconfirmed admin fixes on workspaces where this person's
    /// reviewer key holds reviewer, with the acts it may take on each; or
    /// confirm or revert one.
    Fixes {
        /// Confirm this fix; journals admin.fix_confirmed.
        #[arg(long, conflicts_with = "revert")]
        confirm: Option<String>,
        /// Revert this fix: its text is served again as a new version this
        /// key approves; journals admin.fix_reverted.
        #[arg(long)]
        revert: Option<String>,
    },
}

#[derive(Args)]
struct ReviewListArgs {
    /// Only this workspace's subtree, by id or name.
    #[arg(long)]
    workspace: Option<String>,
    /// Only rows written by this author key (id or name).
    #[arg(long)]
    author_key: Option<String>,
    /// Only claim keys starting with this literal text.
    #[arg(long)]
    claim_key_prefix: Option<String>,
    /// Rows per page, 1 to 200.
    #[arg(long, default_value_t = 50)]
    limit: u32,
    /// Fetch every page.
    #[arg(long)]
    all: bool,
    /// Print only ids, one per line, for --ids.
    #[arg(long)]
    ids_only: bool,
}

#[derive(Subcommand)]
enum AuditCommand {
    /// Verify the exported chain using chain-format.md; fails if the server has
    /// no export. The events tail is only a filtered feed.
    VerifyChain(VerifyChainArgs),
}

#[derive(Args)]
struct VerifyChainArgs {
    /// Verify full history from genesis.
    #[arg(long)]
    genesis: bool,
    /// Deployment signing public key (64 hex chars for 32 raw ed25519 bytes);
    /// repeatable. Confirm the authenticated key set out of band (chain-format.md).
    #[arg(long = "signing-key", env = "OML_CLI_SIGNING_KEY")]
    signing_keys: Vec<String>,
    /// Tenant (id or name); defaults to the key's tenant from GET /v2/whoami.
    #[arg(long)]
    tenant: Option<String>,
}

/// `<workspace>:<role>`, or `<workspace>:<role>:node` for a grant on the node
/// alone (ADR-0044 d4), the workspace by id or name, sent as typed: the
/// server classifies and resolves it, the CLI neither (cli/reference.md
/// conventions; ADR-0026 d4). Neither an id nor a name holds a `:`, so the
/// first one splits the two; the role keeps its `:node` suffix until
/// [`grants_json`] sends it as `covers`.
fn parse_grant(s: &str) -> Result<(String, String), String> {
    let (node, role) = s
        .split_once(':')
        .ok_or_else(|| format!("{s:?} is not <workspace>:<role>"))?;
    // After the role, `node` (ADR-0044 d4) and `create` (ADR-0097 d31), each
    // at most once, in either order.
    let flags: Vec<&str> = role.split(':').skip(1).collect();
    let known = flags.iter().all(|f| matches!(*f, "node" | "create"));
    let once = flags.iter().filter(|f| **f == "node").count() <= 1
        && flags.iter().filter(|f| **f == "create").count() <= 1;
    if known && once {
        Ok((node.to_owned(), role.to_owned()))
    } else {
        Err(format!(
            "{s:?} is not <workspace>:<role>, with :node, :create or both after the role"
        ))
    }
}

/// `--create-workspace <parent>/<name>`: the parent by id or name and the
/// new workspace's name (ADR-0097 d30).
fn parse_new_workspace(s: &str) -> Result<(String, String), String> {
    match s.rsplit_once('/') {
        Some((parent, name)) if !parent.is_empty() && !name.is_empty() => {
            Ok((parent.to_owned(), name.to_owned()))
        }
        _ => Err(format!("{s:?} is not <parent>/<name>")),
    }
}

/// The issuance body's `create_workspaces` (ADR-0097 d30).
fn new_workspaces_json(created: &[(String, String)]) -> Vec<Value> {
    created
        .iter()
        .map(|(parent, name)| json!({ "parent": parent, "name": name }))
        .collect()
}

// ---------------------------------------------------------------------------
// Context resolution
// ---------------------------------------------------------------------------

/// The first start source that sets either value, with the names it gives
/// them; never printed with its values (ADR-0039 d15).
struct Source<'a> {
    /// The credentials file as a refusal names it; `None` for the flags and
    /// the environment, whose value names stand alone.
    file: Option<String>,
    url_name: &'static str,
    key_name: &'static str,
    url: Option<&'a String>,
    key: Option<&'a String>,
}

impl Source<'_> {
    /// Where `name` was set, as doctor reports it: the flag or variable, or
    /// the field and the file.
    fn origin(&self, name: &str) -> String {
        match &self.file {
            Some(file) => format!("{name} in {file}"),
            None => name.to_owned(),
        }
    }

    /// Why a source that sets one value alone is refused, naming the other;
    /// a key without a URL is `no_server`, and names `oml login <server>`
    /// (ADR-0076 d5, d8).
    fn half(&self) -> String {
        let (set, unset) = if self.url.is_some() {
            (self.url_name, self.key_name)
        } else {
            (self.key_name, self.url_name)
        };
        let why = match &self.file {
            Some(file) => format!("{file} sets {set} without {unset}"),
            None => format!("{set} is set without {unset}"),
        };
        if self.url.is_some() {
            why
        } else {
            format!("no_server: {why}; name the server beside it, or run `oml login <server>`")
        }
    }
}

/// Doctor's `url` line when no source names a server: it fails, and no
/// server API is called (ADR-0076 d8).
const NO_SERVER_DOCTOR: &str = "fail no_server: no server URL is configured (--url, OML_URL or \
                                the credentials file); no server API was called, and the \
                                metrics listener is read only where OML_METRICS_URL names it";

/// Doctor's line for an admin on a lead that sets no `oauth.issuer`
/// (ADR-0076 d6, d8).
const SERVER_ORIGIN: &str = "warn server_origin: the server names no public origin (oauth.issuer \
                             unset), so its pages cannot show the URL agents set beside their keys";

/// Doctor's line when the plugin exported its key option and no url: its
/// tools and hooks reach no server, since the url has no default
/// (ADR-0076 d8). `None` otherwise.
fn plugin_no_server(var: &dyn Fn(&str) -> Option<String>) -> Option<&'static str> {
    let set = |name: &str| var(name).is_some_and(|v| !v.is_empty());
    (set(oml_cli::hook::OPTION_API_KEY) && !set(oml_cli::hook::OPTION_URL)).then_some(
        "warn plugin_no_server: the oml plugin has a key and no url; since this release its url \
         has no default, so its tools reach no server and its hooks load nothing",
    )
}

/// Why no start source sets anything.
const NO_PAIR: &str = "no_server: no URL and API key: pass --url and --api-key, set OML_URL and \
                       OML_API_KEY, run `oml login <server>`, or run `oml bootstrap claim`";

/// The flags, then the environment, then the credentials file: the first
/// that sets either value, or `None` (ADR-0039 d15).
fn first_source<'a>(cli: &'a Cli, stored: &'a Credentials) -> Option<Source<'a>> {
    let file = creds::path().map_or_else(
        |_| "the credentials file".to_owned(),
        |p| format!("the credentials file {}", p.display()),
    );
    [
        Source {
            file: None,
            url_name: "--url",
            key_name: "--api-key",
            url: cli.start.flag_url.as_ref(),
            key: cli.start.flag_key.as_ref(),
        },
        Source {
            file: None,
            url_name: "OML_URL",
            key_name: "OML_API_KEY",
            url: cli.start.env_url.as_ref(),
            key: cli.start.env_key.as_ref(),
        },
        Source {
            file: Some(file),
            url_name: "url",
            key_name: "api_key",
            url: stored.url.as_ref(),
            key: stored.api_key.as_ref(),
        },
    ]
    .into_iter()
    .find(|s| s.url.is_some() || s.key.is_some())
}

/// The client of one URL-and-key pair, taken from the first source that sets
/// either value: the flags, then the environment, then the credentials file.
/// A source setting one alone is refused naming the other, so a key is never
/// sent to a URL from another source (ADR-0039 d15).
fn resolve(cli: &Cli, stored: &Credentials) -> Result<Client, CliError> {
    match first_source(cli, stored) {
        Some(Source {
            url: Some(url),
            key: Some(key),
            ..
        }) => Ok(Client::new(url.clone(), Some(key.clone()))),
        Some(source) => Err(missing(&source.half())),
        None => Err(missing(NO_PAIR)),
    }
}

/// The client of `oml bootstrap claim`, which sends no key: the URL of the
/// first source that sets one (ADR-0039 d15).
fn resolve_url(cli: &Cli, stored: &Credentials) -> Result<Client, CliError> {
    let url = cli
        .start
        .flag_url
        .clone()
        .or_else(|| cli.start.env_url.clone())
        .or_else(|| stored.url.clone())
        .ok_or_else(|| {
            let file = creds::path().map_or_else(
                |_| "the credentials file".to_owned(),
                |p| format!("the credentials file {}", p.display()),
            );
            missing(&format!(
                "no_server: no URL: pass --url, set OML_URL, or have {file} name one (oml login \
                 <server> writes it)"
            ))
        })?;
    Ok(Client::new(url, None))
}

/// A start setting is missing: `why`, the one-source rule, and each v0.3.0
/// variable that is set, named for renaming and never read (ADR-0039 d12,
/// d15).
fn missing(why: &str) -> CliError {
    CliError::local(missing_message(why))
}

/// [`missing`]'s message, which doctor prints as its next action.
fn missing_message(why: &str) -> String {
    let mut message = format!(
        "{why}; a URL and its key come from one source, the flags, the environment or the \
         credentials file (cli/reference.md \"Start settings\")"
    );
    for (old, new) in [
        ("OML_CLI_SERVER", "OML_URL"),
        ("OML_CLI_KEY", "OML_API_KEY"),
    ] {
        if std::env::var_os(old).is_some() {
            let _ = write!(message, "; {old} is not read since v0.4.0: rename it {new}");
        }
    }
    message
}

fn main() -> ExitCode {
    let matches = <Cli as clap::CommandFactory>::command().get_matches();
    let mut cli = match <Cli as clap::FromArgMatches>::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(e) => e.exit(),
    };
    cli.start = Start::of(&cli, &matches);
    match run(&cli) {
        Ok(code) => code,
        Err(CliError::Refused {
            status,
            code,
            message,
            rule,
            envelope,
        }) => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&envelope).unwrap_or_default()
                );
            } else {
                // The rule is printed either way (cli/reference.md conventions).
                eprintln!("refused ({status}): {code} — {message}");
                eprintln!("  rule: {rule}");
            }
            ExitCode::from(1)
        }
        Err(CliError::Local(message) | CliError::Unanswered(message)) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one flat dispatch arm per CLI verb; splitting it would only scatter the verb table"
)]
fn run(cli: &Cli) -> Result<ExitCode, CliError> {
    // The hooks read the plugin's options or OML_URL and OML_API_KEY
    // themselves, so they run before the stored admin credentials are loaded,
    // and ignore --url and --api-key (ADR-0029 d4, ADR-0040 d3).
    if let Command::Hook { command } = &cli.command {
        return Ok(match command {
            HookCommand::Guardrails => run_hook(
                "guardrails",
                hook::DEADLINE,
                "no guardrails were loaded",
                |var, stdin| oml_cli::guardrails::guardrails(var, stdin),
            ),
            HookCommand::PreToolUse => {
                use std::io::Write as _;
                let var = |name: &str| std::env::var(name).ok();
                match oml_cli::guardrails::pre_tool_use(&var, &mut std::io::stdin().lock()) {
                    Ok(document) => {
                        if !document.is_empty() {
                            let _ = writeln!(std::io::stdout(), "{document}");
                        }
                        ExitCode::SUCCESS
                    }
                    Err(why) => {
                        let _ = writeln!(
                            std::io::stderr(),
                            "oml hook pre-tool-use: {why}, so the call is blocked"
                        );
                        ExitCode::from(2)
                    }
                }
            }
            HookCommand::SessionStart => run_hook(
                "session-start",
                hook::DEADLINE,
                "no context was loaded",
                |var, stdin| hook::session_start(var, stdin),
            ),
            HookCommand::ToolFailure => run_hook(
                "tool-failure",
                hook::FAILURE_DEADLINE,
                "nothing was recalled",
                |var, stdin| hook::tool_failure(var, stdin),
            ),
            HookCommand::UserPrompt => run_hook(
                "user-prompt",
                std::time::Duration::from_millis(oml_cli::chat::BUDGET_MAX_MS + 1000),
                "nothing was recalled",
                |var, stdin| {
                    oml_cli::events::dispatch(oml_cli::events::Event::UserPromptSubmit, var, stdin)
                },
            ),
            HookCommand::Stop => run_hook(
                "stop",
                hook::SESSION_END_DEADLINE,
                "nothing was captured",
                |var, stdin| oml_cli::events::dispatch(oml_cli::events::Event::Stop, var, stdin),
            ),
            HookCommand::PreCompact => run_hook(
                "pre-compact",
                hook::SESSION_END_DEADLINE,
                "the compaction was not recorded",
                |var, stdin| {
                    oml_cli::events::dispatch(oml_cli::events::Event::PreCompact, var, stdin)
                },
            ),
            HookCommand::SessionEnd => run_hook(
                "session-end",
                hook::SESSION_END_DEADLINE,
                "the session cache was left",
                |var, stdin| {
                    hook::session_end(var, stdin)
                        .map(|()| String::new())
                        .map_err(Some)
                },
            ),
        });
    }
    let stored = creds::load();
    match &cli.command {
        Command::Bootstrap {
            command:
                BootstrapCommand::Claim {
                    key_name,
                    token_stdin: _,
                    tenant_name,
                    principal,
                    person,
                    email,
                },
        } => {
            let client = resolve_url(cli, &stored)?;
            let token = read_secret(&format!("enrollment token for {}", client.server()))?;
            let mut body = json!({ "enrollment_token": token, "name": key_name });
            if let Some(name) = tenant_name {
                body["tenant_name"] = json!(name);
            }
            if let Some(p) = principal {
                body["admin_principal"] = json!(p);
            }
            body["person"] = json!(person);
            body["email"] = json!(email);
            let (_, response) = client.post(
                "/v2/bootstrap/claim",
                &body,
                &CallOpts {
                    auth: false,
                    idempotency: true,
                },
            )?;
            let path = creds::store(&Credentials {
                url: Some(client.server().to_owned()),
                api_key: response["value"].as_str().map(ToOwned::to_owned),
            })
            .map_err(CliError::Local)?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "enrollment token accepted — consumed (single-use; a second claim is refused)"
                );
                println!("first admin key — SHOWN ONCE, NOT RECOVERABLE, NOT JOURNALLED AS VALUE");
                println!();
                println!("    key name: {}", field(&response, "name"));
                println!("    key id  : {}", field(&response, "key_id"));
                println!("    key     : {}", field(&response, "value"));
                println!("    tenant  : {}", field(&response, "tenant_id"));
                println!("    root wa : {}", field(&response, "root_workspace_id"));
                println!();
                println!(
                    "credentials stored in {} (owner-only); the server keeps only the key hash.",
                    path.display()
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Tenants {
            command: TenantsCommand::Create { name },
        } => {
            // The PoC lab exposes no tenant-creation endpoint: the operator
            // tier is out of slice and the standalone deployment hosts the
            // one tenant `oml bootstrap claim` mints (ADR-0023 d16).
            Err(CliError::local(format!(
                "`oml tenants create {name}` needs the operator tier; this standalone deployment \
                 hosts only the tenant created by `oml bootstrap claim`"
            )))
        }

        Command::Keys {
            command: KeysCommand::Issue(args),
        } => {
            let client = resolve(cli, &stored)?;
            // The file is created before anything is sent, so a path that
            // cannot hold the value fails while no key exists (#528).
            let key_file = match &args.out {
                Some(path) => Some(KeyFile::create(path).map_err(CliError::Local)?),
                None => None,
            };
            let grants = with_reader_beside_submitters(&args.grants, &args.role);
            if !cli.json {
                for (node, _) in grants.iter().filter(|g| !args.grants.contains(g)) {
                    eprintln!(
                        "adding {node}:reader beside {node}:submitter \
                         (a contributor recalls; --role submitter writes without recall)"
                    );
                }
            }
            let mut body = json!({
                "role": args.role,
                "name": args.key_name,
                "used_by": args.used_by,
                "grants": grants_json(&grants),
            });
            if !args.create_workspaces.is_empty() {
                body["create_workspaces"] = json!(new_workspaces_json(&args.create_workspaces));
            }
            if let Some(wa) = &args.default_workspace {
                body["default_workspace"] = json!(wa);
            }
            if let Some(crew) = &args.crew {
                body["crew"] = json!(crew);
            }
            if let Some(user) = &args.user {
                body["user"] = json!(user);
            }
            if let Some(person) = &args.person {
                body["person"] = json!(person);
                body["used_by"] = json!("person");
            }
            if let Some(p) = &args.principal {
                body["principal"] = json!(p);
            }
            if let Some(l) = &args.label {
                body["label"] = json!(l);
            }
            if let Some(profile) = &args.profile {
                body["profile"] = json!(profile);
            }
            if let Some(scope) = &args.guardrail_scope {
                body["guardrail_scope"] = json!(scope);
            }
            if let Some(reason) = &args.reason {
                body["reason"] = json!(reason);
            }
            args.limits.add_to(&mut body);
            let mut response = match client.post("/v2/keys", &body, &write_opts()) {
                Ok((_, response)) => response,
                Err(e) => {
                    // The empty file holds nothing either way, so it goes.
                    if let Some(Err(note)) = key_file.map(KeyFile::discard) {
                        eprintln!("note: {note}");
                    }
                    return Err(e);
                }
            };
            // With --out the value goes to the file alone (#528).
            let shown = match key_file {
                Some(file) => {
                    let shown = file.shown().to_owned();
                    let value = response["value"].as_str().unwrap_or_default().to_owned();
                    file.write_value(&value).map_err(|e| {
                        CliError::local(format!(
                            "key {} ({}) was issued, but writing {shown} failed ({e}); its value                              is lost and the key is live. Revoke it with oml keys revoke {}",
                            field(&response, "name"),
                            field(&response, "key_id"),
                            field(&response, "key_id"),
                        ))
                    })?;
                    if let Some(answer) = response.as_object_mut() {
                        answer.remove("value");
                        answer.insert("key_file".to_owned(), json!(shown));
                    }
                    Some(shown)
                }
                None => None,
            };
            if cli.json {
                print_json(&response);
            } else {
                match &shown {
                    Some(path) => println!("key issued — its value is only in {path} (0600)"),
                    None => println!("key issued — SHOWN ONCE"),
                }
                println!();
                println!("    name   : {}", field(&response, "name"));
                println!("    key id : {}", field(&response, "key_id"));
                match &shown {
                    Some(path) => println!("    file   : {path}"),
                    None => println!("    key    : {}", field(&response, "value")),
                }
                println!("    role   : {}", field(&response, "role"));
                println!("    default: {}", field(&response, "default_workspace_id"));
                if response["user"].is_object() {
                    println!("    user   : {}", field(&response["user"], "name"));
                }
                for line in key_limit_lines(&response) {
                    println!("    {line}");
                }
                if let Some(grants) = response["grants"].as_array() {
                    for g in grants {
                        println!(
                            "    grant  : {} : {}{}",
                            field(g, "workspace_node_id"),
                            field(g, "role"),
                            node_suffix(g)
                        );
                    }
                }
                println!();
                if shown.is_none() {
                    println!("store this value now; the server keeps only its hash.");
                }
                // A widening admin act waits on a second admin person
                // (ADR-0097 d5, d23).
                if response["admin_act"]["state"] == "pending_confirmation" {
                    println!(
                        "waits on another admin: the key authenticates nothing until a second \
                         admin person runs `oml admin confirm {}`",
                        field(&response["admin_act"], "act_id")
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Keys {
            command: KeysCommand::LinkSelf { person },
        } => {
            let client = resolve(cli, &stored)?;
            let (_, response) = client.post(
                "/v2/keys/self/link",
                &json!({ "user": person }),
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "key {} linked to {person}, pending: {person} accepts it on the console's Keys page",
                    field(&response, "name")
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Keys {
            command: KeysCommand::Revoke { key_ref, reason },
        } => {
            let client = resolve(cli, &stored)?;
            let path = format!("/v2/keys/{}/revoke", segment(key_ref)?);
            let body = match reason {
                Some(reason) => json!({ "reason": reason }),
                None => json!({}),
            };
            let (_, response) = client.post(&path, &body, &write_opts())?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "key {} revoked at {}",
                    field(&response, "name"),
                    field(&response, "revoked_at")
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Keys {
            command: KeysCommand::IssueAgent(args),
        } => run_issue_agent(cli, &stored, args),

        Command::Keys {
            command: KeysCommand::Show { key_ref },
        } => {
            let client = resolve(cli, &stored)?;
            let (_, response) = client.get(&format!("/v2/keys/{}", segment(key_ref)?))?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "key {} ({})",
                    field(&response, "name"),
                    field(&response, "key_id")
                );
                println!("    role     : {}", field(&response, "role"));
                println!("    principal: {}", field(&response, "principal"));
                println!("    used by  : {}", field(&response, "used_by"));
                println!("    crew     : {}", field(&response, "crew_id"));
                println!("    user     : {}", field(&response["user"], "name"));
                println!(
                    "    default  : {}",
                    field(&response, "default_workspace_id")
                );
                for g in response["grants"].as_array().into_iter().flatten() {
                    println!(
                        "    grant    : {} : {}{}",
                        field(g, "workspace_node_id"),
                        field(g, "role"),
                        node_suffix(g)
                    );
                }
                for d in response["denials"].as_array().into_iter().flatten() {
                    println!(
                        "    denial   : {} ({})",
                        field(d, "workspace_node_id"),
                        field(d, "denial_id")
                    );
                }
                println!("    epoch    : {}", response["epoch"]);
                // A revoked key's name still resolves, so the read says
                // whether the key is history (ADR-0026 d10); a key past its
                // expiry is not revoked and not live either (ADR-0079 d3).
                match response["revoked_at"].as_str() {
                    Some(at) => println!("    revoked  : {at}"),
                    None if response["expired"] == true => {
                        println!("    revoked  : no (expired)");
                    }
                    None => println!("    revoked  : no (live)"),
                }
                // The key's end date, saying when it has passed, and its
                // own rate (ADR-0079 d3, d4).
                for line in key_limit_lines(&response) {
                    println!("    {line}");
                }
                println!("    credential: {}", field(&response, "credential"));
                // The one connect code that can still open a connection,
                // named and never shown; a reviewer code says so (ADR-0067 d3).
                if let Some(code) = response["live_connect_code"].as_object() {
                    let kind = if code.get("extended").and_then(Value::as_bool) == Some(true) {
                        "reviewer code, outlives ten minutes"
                    } else {
                        "single use"
                    };
                    println!(
                        "    code     : {} expires {} ({kind})",
                        code.get("code_id").and_then(Value::as_str).unwrap_or("-"),
                        code.get("expires_at")
                            .and_then(Value::as_str)
                            .unwrap_or("-"),
                    );
                }
                // A connector key's connections, by id: the id is what
                // `oml keys disconnect` names (ADR-0031 d11).
                for c in response["connections"].as_array().into_iter().flatten() {
                    println!(
                        "    connection: {} {} {} -> {}",
                        field(c, "connection_id"),
                        field(c, "state"),
                        field(c, "client_id"),
                        field(c, "redirect_uri"),
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Keys {
            command: KeysCommand::ConnectCode { key_ref, ttl },
        } => {
            let client = resolve(cli, &stored)?;
            let path = format!("/v2/keys/{}/connect-codes", segment(key_ref)?);
            let body = match ttl {
                Some(ttl) => json!({ "ttl_seconds": parse_ttl(ttl).map_err(CliError::Local)? }),
                None => json!({}),
            };
            let (response, replayed) = client.post_replayable(&path, &body, &write_opts())?;
            let printed = minted_code_printed(key_ref, &response, replayed, cli.json);
            for line in &printed.stdout {
                println!("{line}");
            }
            for line in &printed.stderr {
                eprintln!("{line}");
            }
            Ok(ExitCode::from(printed.exit))
        }

        Command::Keys {
            command:
                KeysCommand::Rotate {
                    key_ref,
                    grace,
                    out,
                },
        } => {
            if cli.json && out.is_some() {
                return Err(CliError::local(
                    "--out writes the value to a file and prints none; drop --json",
                ));
            }
            let body = match grace {
                Some(grace) => {
                    let secs = parse_ttl(grace)
                        .map_err(|e| CliError::Local(e.replacen("--ttl", "--grace", 1)))?;
                    json!({ "grace_seconds": secs })
                }
                None => json!({}),
            };
            let path = format!("/v2/keys/{}/rotate", segment(key_ref)?);
            let client = resolve(cli, &stored)?;
            // The owner-only file is created and claimed before anything is
            // sent, as `issue-agent --out` does: a path that cannot hold the
            // value fails while the previous value still stands, and no new
            // value is minted that nothing keeps (ADR-0096 d13; ADR-0028 d1).
            let key_file = match out {
                Some(out) => Some(KeyFile::create(out).map_err(CliError::Local)?),
                None => None,
            };
            let response = match client.post_keyed(&path, &body, &new_idempotency_key()) {
                Ok((_, response)) => response,
                Err(failure) => {
                    if let Some(Err(e)) = key_file.map(KeyFile::discard) {
                        eprintln!("note: {e}");
                    }
                    return Err(failure);
                }
            };
            let value = response["value"].as_str().unwrap_or_default();
            let until = response["previous_valid_until"].as_str().map_or_else(
                || "now: it no longer authenticates".to_owned(),
                |t| t.to_owned(),
            );
            match key_file {
                Some(file) => {
                    let shown = file.shown().to_owned();
                    file.write_value(value).map_err(|e| {
                        CliError::Unanswered(format!(
                            "writing {shown} failed ({e}); key {} was rotated and its new value \
                             is lost: rotate it again",
                            field(&response, "name")
                        ))
                    })?;
                    println!(
                        "rotated — key {}; the new value is in {shown} (owner-only); the previous \
                         value stops at {until}",
                        field(&response, "name"),
                    );
                }
                None if cli.json => print_json(&response),
                None => {
                    println!(
                        "rotated — key {} ({})",
                        field(&response, "name"),
                        field(&response, "key_id")
                    );
                    println!("  value    : {value}");
                    println!("  shown once: store it now; OML keeps only its hash");
                    println!("  previous : stops at {until}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Keys {
            command:
                KeysCommand::Limit {
                    key_ref,
                    expires,
                    no_expiry,
                    requests_per_minute,
                    no_rate_limit,
                },
        } => {
            let body = key_limits_body(
                expires.as_deref(),
                *no_expiry,
                *requests_per_minute,
                *no_rate_limit,
            )
            .map_err(CliError::Local)?;
            let client = resolve(cli, &stored)?;
            let (_, response) = client.post(
                &format!("/v2/keys/{}/limits", segment(key_ref)?),
                &body,
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "limits journalled — key {} ({})",
                    field(&response, "name"),
                    field(&response, "key_id")
                );
                for line in key_limit_lines(&response) {
                    println!("    {line}");
                }
                if response["expiry"].is_null() {
                    println!("    expiry   : none (the key does not expire)");
                }
                if response["requests_per_minute"].is_null() {
                    println!("    rate     : none of its own");
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Keys {
            command:
                KeysCommand::Disconnect {
                    key_ref,
                    connection_id,
                    reason,
                },
        } => {
            let client = resolve(cli, &stored)?;
            let path = format!(
                "/v2/keys/{}/connections/{}/revoke",
                segment(key_ref)?,
                segment(connection_id)?
            );
            let body = match reason {
                Some(reason) => json!({ "reason": reason }),
                None => json!({}),
            };
            let (_, response) = client.post(&path, &body, &write_opts())?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "connection {} of key {key_ref}: {} ({})",
                    field(&response, "connection_id"),
                    field(&response, "state"),
                    field(&response, "revoked_cause"),
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Deny {
            command:
                DenyCommand::Set {
                    key_ref,
                    subtree,
                    reason,
                },
        } => {
            let client = resolve(cli, &stored)?;
            let mut body = json!({ "workspace": subtree });
            if let Some(reason) = reason {
                body["reason"] = json!(reason);
            }
            let (_, response) = client.post(
                &format!("/v2/keys/{}/denials", segment(key_ref)?),
                &body,
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "denial journalled — key {key_ref} no longer reaches {} (epoch {})",
                    field(&response, "workspace_node_id"),
                    response["epoch"]
                );
                println!("  denial id : {}", field(&response, "denial_id"));
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Workspaces {
            command: WorkspacesCommand::Create { parent, name, crew },
        } => {
            let client = resolve(cli, &stored)?;
            let mut body = json!({ "parent": parent, "name": name });
            if let Some(crew) = crew {
                body["crew"] = json!(crew);
            }
            let (_, response) = client.post("/v2/workspaces", &body, &write_opts())?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "created {} ({}) {} (status: {})",
                    field(&response, "name"),
                    field(&response, "id"),
                    field(&response, "path"),
                    field(&response, "status")
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Chat {
            command: ChatCommand::Init { parent },
        } => run_chat_init(cli, &stored, parent.as_deref()),

        Command::Workspaces {
            command: WorkspacesCommand::Status { command },
        } => run_workspace_status(cli, &stored, command),

        Command::Workspaces {
            command:
                WorkspacesCommand::Import {
                    file,
                    into,
                    identity,
                    expect_key,
                    expect_tenant,
                    dry_run,
                    allow_older,
                    upload,
                },
        } => run_workspaces_import(
            cli,
            &stored,
            &ImportArgs {
                file,
                into,
                identity,
                expect_key: expect_key.as_deref(),
                expect_tenant: expect_tenant.as_deref(),
                dry_run: *dry_run,
                allow_older: *allow_older,
                upload: upload.as_deref(),
            },
        ),

        Command::Edges { command } => run_edges(cli, &stored, command),

        Command::Workspaces {
            command:
                WorkspacesCommand::Tree {
                    positional_root,
                    root,
                },
        } => run_workspaces_tree(
            cli,
            &stored,
            positional_root
                .as_deref()
                .or(root.as_deref())
                .unwrap_or("universe"),
        ),

        Command::Workspaces {
            command: WorkspacesCommand::Show { workspace },
        } => {
            let client = resolve(cli, &stored)?;
            let (_, response) = client.get(&format!("/v2/workspaces/{}", segment(workspace)?))?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "workspace {} ({})",
                    field(&response, "name"),
                    field(&response, "id")
                );
                println!("    path    : {}", field(&response, "path"));
                println!("    parent  : {}", field(&response, "parent_id"));
                println!("    status  : {}", field(&response, "status"));
                // ADR-0030 d13: the mode, and the root a node inside lies under.
                println!("    mode    : {}", field(&response, "mode"));
                if let Some(root) = response["in_progress_root"].as_object() {
                    println!(
                        "    inside  : {} (in progress; unreviewed memories are recalled here)",
                        root.get("name").and_then(Value::as_str).unwrap_or("?")
                    );
                }
                println!("    crew    : {}", crew_line(&response["crew"]));
                // ADR-0098 d9: the flag, since when, and a set that waits.
                println!("    public  : {}", public_line(&response["public"]));
                for c in response["children"].as_array().into_iter().flatten() {
                    println!(
                        "    child   : {} ({}, {})",
                        field(c, "path_leaf"),
                        field(c, "id"),
                        field(c, "status")
                    );
                }
                let verbs: Vec<&str> = response["my_effective_verbs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                println!(
                    "    my verbs: {}",
                    if verbs.is_empty() {
                        "none".to_owned()
                    } else {
                        verbs.join(", ")
                    }
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Workspaces {
            command:
                WorkspacesCommand::Federation {
                    workspace,
                    accept,
                    reason,
                },
        } => {
            let client = resolve(cli, &stored)?;
            let mut body = json!({ "accept_federated": accept == "on" });
            if let Some(reason) = reason {
                body["reason"] = json!(reason);
            }
            let (status, response) = client.post(
                &format!("/v2/workspaces/{}/federation", segment(workspace)?),
                &body,
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else if status == 202 {
                println!(
                    "{}",
                    federation_pending(&format!("workspace {workspace}"), &response)
                );
            } else {
                println!(
                    "workspace {workspace} {} federated reads",
                    if response["accept_federated"] == true {
                        "accepts"
                    } else {
                        "declines"
                    }
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Workspaces {
            command: WorkspacesCommand::Public { command },
        } => {
            let (workspace, public, reason) = match command {
                WorkspacePublicCommand::Set { workspace, reason } => {
                    (workspace, true, Some(reason.as_str()))
                }
                WorkspacePublicCommand::Clear { workspace, reason } => {
                    (workspace, false, reason.as_deref())
                }
            };
            let client = resolve(cli, &stored)?;
            let (status, response) = client.post(
                &format!("/v2/workspaces/{}/public", segment(workspace)?),
                &json!({ "public": public, "reason": reason }),
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!("{}", public_printed(workspace, status, &response));
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Workspaces {
            command: WorkspacesCommand::Mode { workspace, mode },
        } => {
            let client = resolve(cli, &stored)?;
            let (_, response) = client.post(
                &format!("/v2/workspaces/{}/mode", segment(workspace)?),
                &json!({ "mode": mode }),
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "workspace {} ({}) is {}",
                    field(&response, "name"),
                    field(&response, "id"),
                    field(&response, "mode"),
                );
                if mode == "in_progress" {
                    println!(
                        "readers inside now recall unreviewed memories; commit with: oml workspaces mode {} standard",
                        field(&response, "name")
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Workspaces {
            command:
                WorkspacesCommand::Export {
                    workspace,
                    recipients,
                    out,
                    history,
                    release,
                    since,
                },
        } => {
            // A bundle is never written unencrypted (ADR-0084 d5): refused
            // here before anything is sent or created.
            if recipients.is_empty() {
                return Err(CliError::local(
                    "bundle.unencrypted: name at least one --recipient <age1…>; a bundle is never \
                     written unencrypted; nothing was sent",
                ));
            }
            let client = resolve(cli, &stored)?;
            let path = format!("/v2/workspaces/{}/exports", segment(workspace)?);
            let mut body = json!({ "recipients": recipients });
            if *history {
                body["mode"] = json!("history");
            }
            if let Some(release) = release {
                body["mode"] = json!("release");
                body["release"] = json!(release);
            }
            if let Some(since) = since {
                body["since"] = json!(since);
            }
            let (bytes, digest) = oml_cli::export::download(&client, &path, &body, out)?;
            if cli.json {
                print_json(&json!({
                    "out": out.display().to_string(),
                    "bytes": bytes,
                    "manifest_digest": digest,
                }));
            } else {
                println!(
                    "wrote {} ({bytes} bytes, owner-only), manifest {}",
                    out.display(),
                    digest.as_deref().unwrap_or("unknown"),
                );
                println!(
                    "the file is outside every server's erasure: whoever holds it and a \
                     recipient's identity reads it"
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Workspaces {
            command:
                WorkspacesCommand::Crew {
                    command: WorkspaceCrewCommand::Set { workspace, crew },
                },
        } => {
            let client = resolve(cli, &stored)?;
            let (_, response) = client.post(
                &format!("/v2/workspaces/{}/crew", segment(workspace)?),
                &json!({ "crew": crew }),
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "workspace {} ({}) names crew {}",
                    field(&response, "name"),
                    field(&response, "id"),
                    crew_line(&response["crew"])
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Crews { command } => run_crews(cli, &stored, command),
        Command::Web { command } => run_web(cli, &stored, command),
        Command::Search { command } => run_search(cli, &stored, command),
        Command::Usage { command } => run_usage(cli, &stored, command),
        Command::Admin { command } => run_admin(cli, &stored, command),

        Command::Write(args) => {
            refuse_skill_fields_on_another_kind(args);
            run_write(cli, &stored, &Write::of(args))
        }
        Command::Read(args) => run_read(cli, &stored, args),
        Command::Withdraw(args) => run_withdraw(cli, &stored, args),
        Command::Events(args) => run_events(cli, &stored, args),
        Command::Review { command } => run_review(cli, &stored, command),

        Command::Audit {
            command: AuditCommand::VerifyChain(args),
        } => run_verify_chain(cli, &stored, args),

        Command::Guardrails { command } => run_guardrails(cli, &stored, command),
        Command::Skills {
            command:
                SkillsCommand::List {
                    workspace,
                    include_own_held,
                },
        } => run_skills_list(cli, &stored, workspace.as_deref(), *include_own_held),
        Command::Login { target } => run_login(cli, target),
        Command::Doctor => run_doctor(cli, &stored),
        Command::Sync(args) => run_sync(cli, &stored, args),
        Command::Anchor(args) => {
            let cwd = std::env::current_dir()
                .map_err(|e| CliError::local(format!("the current directory: {e}")))?;
            let adapter = args
                .adapter
                .as_deref()
                .map(oml_cli::sync::adapter::Kind::parse)
                .transpose()
                .map_err(CliError::Local)?;
            let anchor = oml_cli::sync::anchor::anchor(
                &cwd,
                &args.target,
                &args.rev,
                args.repo.as_deref(),
                adapter,
            )
            .map_err(CliError::Local)?;
            print_json(&anchor);
            Ok(ExitCode::SUCCESS)
        }
        Command::Quickstart(args) => run_quickstart(args),

        Command::Demo {
            command: DemoCommand::Seed { workspace, parent },
        } => {
            let client = resolve(cli, &stored)?;
            let report = oml_cli::demo::seed(&client, workspace, parent)?;
            if cli.json {
                print_json(&json!({
                    "workspace": report.workspace,
                    "approved": report.approved,
                    "superseded": report.superseded,
                    "held": report.held,
                    "fenced_identities": report.identities,
                }));
            } else {
                println!(
                    "seeded {}: {} approved ({} of them replaced by an approved newer version), {} held",
                    report.workspace, report.approved, report.superseded, report.held
                );
                println!(
                    "fenced from the whole tree: {} (their journal entries stay)",
                    report.identities.join(", ")
                );
                println!(
                    "next: oml keys issue-agent <name> --principal crew/<name> --workspace {} --connect-code",
                    report.workspace
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Setup {
            command: SetupCommand::ClaudeCode(args),
        } => run_setup_claude_code(cli, &stored, args),
        Command::Hook { .. } => unreachable!("the hook ran before the credentials were loaded"),
    }
}

/// `oml guardrails show` and `oml guardrails export`: one `GET
/// /v1/guardrails` under the stored key, rendered by `oml-guardrail` as the
/// hook renders it (proposed ADR-0037 d7, d11).
fn run_guardrails(
    cli: &Cli,
    stored: &Credentials,
    command: &GuardrailsCommand,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let workspace = match command {
        GuardrailsCommand::Show { workspace } | GuardrailsCommand::Export { workspace, .. } => {
            workspace
        }
    };
    let path = match workspace {
        Some(w) => format!("/v1/guardrails?workspace={}", encode(w)),
        None => "/v1/guardrails".to_owned(),
    };
    let (_, set) = client.get(&path)?;
    let origin = hook::origin(client.server());
    match command {
        GuardrailsCommand::Show { .. } => {
            let (text, over) =
                oml_cli::guardrails::show_text(&set, &origin).map_err(CliError::local)?;
            println!("{text}");
            Ok(if over {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
        GuardrailsCommand::Export {
            format, out, check, ..
        } => {
            let exported = oml_cli::guardrails::export(&set, &origin, format, out, *check)
                .map_err(CliError::local)?;
            Ok(if exported {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
    }
}

/// `oml skills list`: `GET /v1/skills` under the stored key, every page,
/// one line per offered skill in the read's order, no bodies (ADR-0080
/// d11). With `--json`, the scope and every page's skills as one document.
fn run_skills_list(
    cli: &Cli,
    stored: &Credentials,
    workspace: Option<&str>,
    include_own_held: bool,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let mut query = Vec::new();
    if let Some(w) = workspace {
        query.push(format!("workspace={}", encode(w)));
    }
    if include_own_held {
        query.push("include_own_held=true".to_owned());
    }
    let mut skills: Vec<Value> = Vec::new();
    let mut cursor: Option<String> = None;
    let first = loop {
        let mut params = query.clone();
        if let Some(c) = &cursor {
            params.push(format!("cursor={}", encode(c)));
        }
        let path = if params.is_empty() {
            "/v1/skills".to_owned()
        } else {
            format!("/v1/skills?{}", params.join("&"))
        };
        let (_, page) = client.get(&path)?;
        skills.extend(page["skills"].as_array().cloned().unwrap_or_default());
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break page,
        }
    };
    if cli.json {
        println!(
            "{}",
            json!({
                "scope_path": first["scope_path"],
                "guardrail_scope": first["guardrail_scope"],
                "set_digest": first["set_digest"],
                "skills": skills,
            })
        );
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "skills offered at {} (guardrail scope {}): {}",
        oml_cli::skills::printable(first["scope_path"].as_str().unwrap_or("?")),
        first["guardrail_scope"].as_str().unwrap_or("?"),
        skills.len()
    );
    for s in &skills {
        println!("{}", oml_cli::skills::list_line(s));
    }
    Ok(ExitCode::SUCCESS)
}

/// The doctor's local skill lines (ADR-0080 d15): whether the `oml` on
/// `PATH`, which the plugin's hooks run, asks before a `Write` under
/// `~/.claude/skills/` (probed with a call it only reads), and how many
/// skills the current project's and the home's `.claude/skills/` keep on
/// disk.
fn local_skill_lines() -> Vec<String> {
    let home = oml_cli::guardrails::skills_home(&|name| std::env::var(name).ok())
        .filter(|h| Path::new(h).is_absolute());
    let mut lines = Vec::new();
    if let Some(home) = &home {
        let input = json!({
            "session_id": "oml-doctor-skill-probe",
            "hook_event_name": "PreToolUse",
            "tool_name": "Write",
            "tool_input": { "file_path": format!("{home}/.claude/skills/oml-doctor/SKILL.md"), "content": "" },
            "cwd": home,
        });
        let probed = oml_cli::skills::probe(
            "oml",
            &["hook", "pre-tool-use"],
            &input.to_string(),
            oml_cli::skills::PROBE_TIMEOUT,
        );
        lines.extend(oml_cli::skills::hook_probe_line(&probed));
    }
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join(".claude").join("skills"));
    }
    if let Some(home) = home {
        roots.push(PathBuf::from(home).join(".claude").join("skills"));
    }
    lines.extend(oml_cli::skills::disk_lines(&roots));
    lines
}

/// One `oml hook` verb: the variables and stdin in, the hook document or
/// why there is none out (`None` is silent).
type HookVerb =
    fn(&dyn Fn(&str) -> Option<String>, &mut dyn std::io::Read) -> Result<String, Option<String>>;

/// `oml hook <verb>`: the hook document on stdout, or nothing, and at most
/// one line on stderr; exit 0 whatever happened, so a session always starts
/// and a failed command is never followed by a hook error (ADR-0029 d5,
/// ADR-0038 d7). The hook runs on its own thread under `deadline`:
/// returning from `main` ends the process even while that thread is stuck
/// where no timeout reaches, as in a DNS lookup.
fn run_hook(
    verb: &'static str,
    deadline: std::time::Duration,
    nothing: &'static str,
    hook: HookVerb,
) -> ExitCode {
    use std::io::Write as _;
    let (done, outcome) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let var = |name: &str| std::env::var(name).ok();
        let _ = done.send(hook(&var, &mut std::io::stdin().lock()));
    });
    let reason = match outcome.recv_timeout(deadline) {
        Ok(Ok(document)) => {
            // A closed stream is not a reason to exit non-zero: writes are
            // not unwrapped, as println! would.
            if !document.is_empty() {
                let _ = writeln!(std::io::stdout(), "{document}");
            }
            None
        }
        Ok(Err(reason)) => reason,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Some(format!(
            "no answer within {} seconds, so {nothing}",
            deadline.as_secs()
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Some(format!("the hook stopped before it finished, so {nothing}"))
        }
    };
    if let Some(reason) = reason {
        let _ = writeln!(std::io::stderr(), "oml hook {verb}: {reason}");
    }
    ExitCode::SUCCESS
}

/// `oml chat init`: creates `chat` under the key's user root, or under
/// `parent`, and prints the value the plugin reads; an existing one is
/// named, not an error. Then asks for ADR-0030's in-progress mode where the
/// server serves it (ADR-0063 d8).
fn run_chat_init(
    cli: &Cli,
    stored: &Credentials,
    parent: Option<&str>,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let mut body = json!({ "name": oml_cli::chat::CHAT_WORKSPACE });
    if let Some(parent) = parent {
        body["parent"] = json!(parent);
    }
    let created = match client.post("/v2/workspaces", &body, &write_opts()) {
        Ok((_, response)) => response,
        Err(CliError::Refused { code, .. }) if code == "name.taken" => {
            // The name is reserved for a person's key (d8), but an existing
            // `chat` is adopted only where a person's chat workspace sits:
            // directly under this key's user root, never elsewhere.
            let (_, existing) = client.get(&format!(
                "/v2/workspaces/{}",
                segment(oml_cli::chat::CHAT_WORKSPACE)?
            ))?;
            let me = client.whoami()?;
            let user = me["user"]["name"].as_str().unwrap_or_default();
            let path = existing["path"].as_str().unwrap_or_default();
            if let Err(why) = oml_cli::chat::adopt(path, user) {
                return Err(CliError::local(format!("oml chat init: {why}")));
            }
            println!(
                "the chat workspace {path} already exists; set OML_CHAT_WORKSPACE={}",
                field(&existing, "id")
            );
            return Ok(ExitCode::SUCCESS);
        }
        Err(e) => return Err(e),
    };
    let id = field(&created, "id").to_owned();
    let mode = client.put(
        &format!("/v2/workspaces/{id}/mode"),
        &json!({ "mode": "in_progress" }),
    );
    if cli.json {
        print_json(&json!({ "workspace": created, "in_progress": mode.is_ok() }));
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "created {} ({}), held facts stay inside it until you review them",
        field(&created, "path"),
        id
    );
    match mode {
        Ok(_) => {
            println!(
                "drafts visible to: everyone working here; your agents read each other's chat facts, labelled unreviewed"
            )
        }
        Err(e) => println!(
            "not in progress ({}): each agent key reads only its own chat facts; retry with \
             `oml workspaces mode {id} in_progress`",
            one_line_error(&e)
        ),
    }
    println!();
    println!("Set these where the plugin runs (or the plugin's chat_workspace option):");
    println!("  OML_CHAT_WORKSPACE={id}");
    println!("  OML_CHAT_CAPTURE=1      # ask the agent to write chat facts at Stop");
    println!("  OML_TURN_RECALL=1       # optional: recall on every prompt");
    Ok(ExitCode::SUCCESS)
}

/// The document lines (ADR-0081 Consequences): whether the server's MCP
/// resources serve documents, then, in a clone, the paths the default
/// workspace holds both as a document and by another adapter, and the
/// document files that now cut past the passage cap. One listing of this
/// repository's document claims, one per document's path (at most 20).
fn document_doctor(client: &Client) -> Vec<String> {
    let mut lines = oml_cli::documents::server_lines(
        &client
            .post(
                "/mcp",
                &json!({"jsonrpc": "2.0", "id": 1, "method": "resources/templates/list"}),
                &CallOpts {
                    auth: true,
                    idempotency: false,
                },
            )
            .map(|(_, v)| v),
    );
    let Ok(git) = oml_cli::sync::source::Git::at(&std::env::current_dir().unwrap_or_default())
    else {
        return lines;
    };
    let Some(repo) = git
        .origin_url()
        .and_then(|url| oml_cli::sync::source::repo_of_remote(&url).ok())
        .map(|r| r.repo)
    else {
        return lines;
    };
    let Ok((_, page)) = client.get(&format!(
        "/v1/memories?kinds=document&anchor_repo={}&limit=100",
        encode(&repo)
    )) else {
        return lines;
    };
    let docs: Vec<Value> = page["items"].as_array().cloned().unwrap_or_default();
    let mut anchored = docs.clone();
    let mut files = Vec::new();
    for doc in docs.iter().take(20) {
        let Some(path) = doc["anchor"]["path"].as_str() else {
            continue;
        };
        if let Ok((_, near)) = client.get(&format!(
            "/v1/memories?anchor_repo={}&anchor_path_prefix={}&limit=100",
            encode(&repo),
            encode(path)
        )) {
            anchored.extend(
                near["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|i| i["kind"] != "document")
                    .cloned(),
            );
        }
        if let Some(text) = oml_cli::documents::read_document_file(git.root(), path) {
            let format = doc["format"].as_str().unwrap_or("text/plain").to_owned();
            files.push((path.to_owned(), text, format));
        }
    }
    lines.extend(oml_cli::documents::mixed_lines(&anchored));
    lines.extend(oml_cli::documents::cap_lines(&files));
    lines
}

/// The chat lines only the server answers (ADR-0063 d13): whether it takes
/// `origin_kinds`, and whether the named chat workspace resolves and is the
/// key's user's own.
fn chat_doctor(client: &Client, me: Option<&Value>) -> Vec<String> {
    let mut out = Vec::new();
    if let Err(CliError::Refused {
        status, envelope, ..
    }) = client.get("/v1/memories?own=true&limit=1&origin_kinds=manual")
    {
        out.extend(oml_cli::chat::origin_probe_line(status, &envelope));
    }
    let var = |name: &str| std::env::var(name).ok();
    if let Some(named) = oml_cli::chat::chat_workspace(&var) {
        let user = me.and_then(|m| m["user"]["name"].as_str());
        match segment(&named).and_then(|s| client.get(&format!("/v2/workspaces/{s}"))) {
            Ok((_, node)) => out.extend(oml_cli::chat::chat_scope_line(node["path"].as_str(), user)),
            Err(_) => out.push(format!(
                "warn chat: {} names {named}, which this key cannot resolve; run oml chat init and use the id it prints",
                oml_cli::chat::WORKSPACE_VAR
            )),
        }
    }
    out
}

fn one_line_error(e: &CliError) -> String {
    match e {
        CliError::Refused { code, .. } => code.clone(),
        CliError::Local(why) => why.lines().next().unwrap_or("").to_owned(),
        #[allow(unreachable_patterns)]
        _ => "refused".to_owned(),
    }
}

fn write_opts() -> CallOpts {
    CallOpts {
        auth: true,
        idempotency: true,
    }
}

fn field<'a>(v: &'a Value, name: &str) -> &'a str {
    v[name].as_str().unwrap_or("—")
}

/// ` (node)` for a grant on its node alone, empty for a subtree grant
/// (ADR-0044 d4).
fn node_suffix(grant: &Value) -> &'static str {
    if grant["covers"] == "node" {
        " (node)"
    } else {
        ""
    }
}

/// `oml workspaces public set|clear`'s line (ADR-0098 d9): `effective`
/// with the flag now held, or that the set waits on another admin person,
/// with the act id either way.
/// A federation widening that waits on another admin person (ADR-0097 d5).
fn federation_pending(what: &str, act: &Value) -> String {
    let id = act["act_id"].as_str().unwrap_or("?");
    format!(
        "{what}: the change waits on another admin person (act {id}); they confirm it with oml admin confirm {id}"
    )
}

fn public_printed(workspace: &str, status: u16, response: &Value) -> String {
    let act = response["act_id"].as_str().unwrap_or("?");
    if status == 202 {
        format!(
            "workspace {workspace}: public waits on another admin person (act {act}); they confirm it with oml admin confirm {act}"
        )
    } else if response["public"] == true {
        format!("workspace {workspace}: public, effective (act {act})")
    } else {
        format!("workspace {workspace}: not public, effective (act {act})")
    }
}

/// `oml workspaces show`'s public line: yes since when, marked
/// `sole_admin` where it took effect so, or no; and a set that waits.
fn public_line(public: &Value) -> String {
    let mut line = if public["value"] == true {
        format!(
            "yes, since {}{}",
            public["since"].as_str().unwrap_or("?"),
            if public["sole_admin"] == true {
                " (sole_admin)"
            } else {
                ""
            }
        )
    } else {
        "no".to_owned()
    };
    if let Some(act) = public["pending_act_id"].as_str() {
        line.push_str(&format!(
            "; a set waits on another admin person (act {act})"
        ));
    }
    line
}

/// A crew record as one line: its name and id, or `—` for none.
fn crew_line(crew: &Value) -> String {
    if crew.is_object() {
        format!("{} ({})", field(crew, "name"), field(crew, "crew_id"))
    } else {
        "—".to_owned()
    }
}

/// The issuance body's `grants` (openapi.yaml `GrantRequest`), each
/// workspace as the operator spelled it.
fn grants_json(grants: &[(String, String)]) -> Vec<Value> {
    grants
        .iter()
        .map(|(node, role)| {
            let mut parts = role.split(':');
            let mut grant = json!({ "workspace": node, "role": parts.next().unwrap_or_default() });
            for flag in parts {
                match flag {
                    // A `:node` suffix is a grant on the node alone (ADR-0044 d4).
                    "node" => grant["covers"] = json!("node"),
                    // `:create` lets the key create children there (ADR-0097 d31).
                    "create" => grant["create"] = json!(true),
                    _ => {}
                }
            }
            grant
        })
        .collect()
}

// ---------------------------------------------------------------------------
// oml sync (ADR-0057)
// ---------------------------------------------------------------------------

/// Authorize a public-server sync and disclose its retention when the operator
/// first opts a source's clone in, or for a one-time opt-in outside a clone
/// (ADR-0057 d6, d17).
fn authorize_public_sync(client: &Client, source: &Path, explicit: bool) -> Result<(), CliError> {
    use oml_cli::sync::source::{
        SharedServerConsent, authorize_shared_server, record_shared_server_consent,
    };
    // A whoami without `shared` comes from a server older than ADR-0076 and
    // counts as not shared: the operator's choice of 2026-11-11 (d9).
    let shared = client.whoami()?["shared"].as_bool().unwrap_or(false);
    let consent = authorize_shared_server(client.server(), shared, source, explicit)
        .map_err(CliError::Local)?;
    let scope = match consent {
        SharedServerConsent::FirstOptIn => "for this server in this clone",
        SharedServerConsent::OneTime => "for this run only; the source is outside a git clone",
        SharedServerConsent::NotRequired | SharedServerConsent::Recorded => return Ok(()),
    };
    eprintln!(
        "warn: shared-server sync allowed {scope} for {}: its admins can read submitted \
         memories; superseding or retiring a memory does not erase submitted text or anchors",
        client.server()
    );
    if consent == SharedServerConsent::FirstOptIn {
        record_shared_server_consent(source, client.server()).map_err(CliError::Local)?;
    }
    Ok(())
}

/// `oml sync`: refuses a shared server unless allowed, reads the source
/// at --rev, and runs the plan through the HTTP API (ADR-0057 d6-d11).
fn run_sync(cli: &Cli, stored: &Credentials, args: &SyncArgs) -> Result<ExitCode, CliError> {
    use oml_cli::sync::{adapter::Kind, api::HttpApi, run};
    if args.adapter.as_deref() == Some("delta") {
        return run_lake_sync(cli, stored, args);
    }
    let client = resolve(cli, stored)?;
    let cwd = std::env::current_dir()
        .map_err(|e| CliError::local(format!("the current directory: {e}")))?;
    let opts = run::Options {
        source: args.source.clone(),
        workspace: args.workspace.clone(),
        adapter: args
            .adapter
            .as_deref()
            .map(Kind::parse)
            .transpose()
            .map_err(CliError::Local)?,
        rev: args.rev.clone(),
        repo: args.repo.clone(),
        dry_run: args.dry_run,
        check: args.check,
        record: args.record,
        json: cli.json,
        max_writes: args.max_writes,
        adopt: args.adopt,
        retire_all: args.retire_all,
        edges: args.edges,
        ratify: args.ratify,
        allow_rollback: args.allow_rollback,
        change_adapter: args.change_adapter,
        exclude: args.exclude.clone(),
        change_exclude: args.change_exclude,
        pr: args.pr,
        base: args.base.clone(),
    };
    let loaded = run::load(&cwd, &opts).map_err(CliError::Local)?;
    let consent_root = loaded.root.as_deref().unwrap_or(&cwd);
    authorize_public_sync(&client, consent_root, args.allow_public_server)?;
    for warning in &loaded.warnings {
        eprintln!("{warning}");
    }
    let api = HttpApi { client };
    let report = run::execute(&api, &loaded, &opts, &mut std::io::stdout())?;
    if !opts.dry_run && !opts.check && !opts.edges && report.exit == 0 {
        run::record_exclude(&loaded, &opts).map_err(CliError::Local)?;
    }
    if cli.json {
        print_json(&report.json);
    }
    Ok(ExitCode::from(report.exit))
}

/// `oml sync --adapter delta`: the lake build reads a Delta Lake's tables from
/// their logs; the default build names the build that does (ADR-0062 d3).
#[cfg(not(feature = "lake-sync"))]
fn run_lake_sync(_: &Cli, _: &Credentials, _: &SyncArgs) -> Result<ExitCode, CliError> {
    Err(CliError::local(
        "--adapter delta requires the oml-lake build; install oml-lake for the same commands \
         with Delta Lake support",
    ))
}

#[cfg(feature = "lake-sync")]
fn run_lake_sync(cli: &Cli, stored: &Credentials, args: &SyncArgs) -> Result<ExitCode, CliError> {
    use oml_cli::lake::run::{self, Options};
    use oml_cli::sync::api::HttpApi;
    let unsupported = [
        (args.pr.is_some(), "--pr"),
        (args.edges, "--edges"),
        (args.ratify, "--ratify"),
        (args.retire_all, "--retire-all"),
        (args.adopt, "--adopt"),
        (args.allow_rollback, "--allow-rollback"),
        (args.change_adapter, "--change-adapter"),
        (!args.exclude.is_empty(), "--exclude"),
        (args.repo.is_some(), "--repo"),
    ];
    if let Some((_, flag)) = unsupported.iter().find(|(set, _)| *set) {
        return Err(CliError::local(format!(
            "{flag} applies only to repository documents, not --adapter delta; remove it"
        )));
    }
    let root = oml_cli::lake::local_root(&args.source).map_err(CliError::Local)?;
    let client = resolve(cli, stored)?;
    authorize_public_sync(&client, &root, args.allow_public_server)?;
    let opts = Options {
        root,
        uri_base: args.uri_base.clone(),
        workspace: args.workspace.clone(),
        dry_run: args.dry_run,
        check: args.check,
        record: args.record,
        json: cli.json,
        max_writes: args.max_writes,
        depth: run::DEPTH,
        at_version: None,
    };
    let api = HttpApi { client };
    let report = run::execute(&api, &opts, &mut std::io::stdout())?;
    if cli.json {
        print_json(&report.json);
    }
    Ok(ExitCode::from(report.exit))
}

// ---------------------------------------------------------------------------
// oml write, oml read, oml events, oml review (ADR-0039 d10)
// ---------------------------------------------------------------------------

/// One write, from `oml write`'s flags.
struct Write<'a> {
    text: &'a str,
    claim_key: Option<&'a str>,
    kind: Option<&'a str>,
    derives_from: &'a [String],
    workspace: Option<&'a str>,
    replaces: Option<&'a str>,
    expected_version: Option<u32>,
    retirement: bool,
    reason: Option<&'a str>,
    idempotency_key: Option<&'a str>,
    severity: Option<&'a str>,
    rule: Option<&'a serde_json::Value>,
    anchor: Option<&'a serde_json::Value>,
    format: Option<&'a str>,
    title: Option<&'a str>,
    origin_kind: Option<&'a str>,
    /// The skill object's name and description (ADR-0080 d1).
    skill: Option<(&'a str, &'a str)>,
}

impl<'a> Write<'a> {
    fn of(args: &'a WriteArgs) -> Self {
        Self {
            text: &args.text,
            claim_key: args.claim_key.as_deref(),
            kind: args.kind.as_deref(),
            derives_from: &args.derives_from,
            workspace: args.workspace.as_deref(),
            replaces: args.replaces.as_deref(),
            expected_version: args.expected_version,
            retirement: args.retirement,
            reason: args.reason.as_deref(),
            idempotency_key: args.idempotency_key.as_deref(),
            severity: args.severity.as_deref(),
            rule: args.rule.as_ref(),
            anchor: args.anchor.as_ref(),
            format: args.format.as_deref(),
            title: args.title.as_deref(),
            origin_kind: args.origin_kind.as_deref(),
            skill: args
                .skill_name
                .as_deref()
                .zip(args.skill_description.as_deref()),
        }
    }
}

/// `--skill-*` beside a `--kind` other than skill, or with no `--kind`,
/// which writes a memory: a usage error, as clap draws one, before anything
/// is sent (ADR-0080 d1: the skill object is refused on every other kind).
fn refuse_skill_fields_on_another_kind(args: &WriteArgs) {
    if args.skill_name.is_some() && args.kind.as_deref() != Some("skill") {
        <Cli as clap::CommandFactory>::command()
            .error(
                clap::error::ErrorKind::ArgumentConflict,
                "--skill-name and --skill-description go with --kind skill alone",
            )
            .exit();
    }
}

/// `--anchor`: one JSON object, inline or `@file`; whether it is a
/// well-formed anchor is the server's grammar to decide (ADR-0057 d3).
fn parse_anchor_arg(text: &str) -> Result<serde_json::Value, String> {
    let source = match text.strip_prefix('@') {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read the anchor file {path}: {e}"))?,
        None => text.to_owned(),
    };
    match serde_json::from_str::<serde_json::Value>(&source) {
        Ok(value @ serde_json::Value::Object(_)) => Ok(value),
        Ok(_) => Err(
            "an anchor is one JSON object, such as {\"repo\":…,\"rev\":…,\"path\":…}".to_owned(),
        ),
        Err(e) => Err(format!("an anchor is one JSON object: {e}")),
    }
}

/// `--rule`: one JSON object, refused before anything is sent otherwise;
/// whether it is an `oml-gr/1` rule is the server's scan to decide
/// (proposed ADR-0037 d8, d10).
fn parse_rule(text: &str) -> Result<serde_json::Value, String> {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value @ serde_json::Value::Object(_)) => Ok(value),
        Ok(_) => Err("a rule is one JSON object, such as {\"rule\":\"oml-gr/1\",…}".to_owned()),
        Err(e) => Err(format!("a rule is one JSON object: {e}")),
    }
}

/// `oml write`: one `POST /v1/memories`, or with `--replaces` one `POST
/// /v1/memories/{replaces}/supersede`, as `oml_write` chooses by the fields
/// present (ADR-0039 d3). Each flag outside its mode is refused by clap
/// before anything is sent; an omitted field is the server's default, so
/// the server derives the claim key and journals learn when lineage is
/// named (ADR-0039 d3, d6).
fn run_write(cli: &Cli, stored: &Credentials, write: &Write<'_>) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    // The text `-` is read from standard input, so a diagram or a fragment
    // is piped in as it is (ADR-0056 d13).
    let piped;
    let text = if write.text == "-" {
        let mut input = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
            .map_err(|e| CliError::local(format!("reading the text from standard input: {e}")))?;
        piped = input;
        piped.as_str()
    } else {
        write.text
    };
    let mut body = json!({ "body": text });
    for (name, value) in [
        ("claim_key", write.claim_key),
        ("kind", write.kind),
        ("workspace", write.workspace),
        ("reason", write.reason),
        ("severity", write.severity),
        ("format", write.format),
        ("title", write.title),
    ] {
        if let Some(value) = value {
            body[name] = json!(value);
        }
    }
    if let Some(rule) = write.rule {
        body["rule"] = rule.clone();
    }
    if let Some((name, description)) = write.skill {
        body["skill"] = json!({ "name": name, "description": description });
    }
    if let Some(anchor) = write.anchor {
        body["anchor"] = anchor.clone();
    }
    if let Some(kind) = write.origin_kind {
        body["origin"] = json!({ "kind": kind });
    }
    if let Some(expected) = write.expected_version {
        body["expected_version"] = json!(expected);
    }
    if !write.derives_from.is_empty() {
        body["derives_from"] = json!(write.derives_from);
    }
    if write.retirement {
        body["retirement"] = json!(true);
    }
    let path = match write.replaces {
        Some(target) => format!("/v1/memories/{}/supersede", segment(target)?),
        None => "/v1/memories".to_owned(),
    };
    let key = write
        .idempotency_key
        .map_or_else(new_idempotency_key, ToOwned::to_owned);
    let (_, response) = client.post_keyed(&path, &body, &key)?;
    if cli.json {
        print_json(&response);
    } else {
        println!(
            "accepted — {} (journal seq {})",
            field(&response, "state"),
            response["journal_seq"]
        );
        println!("  artifact  : {}", field(&response, "artifact_id"));
        println!("  claim_key : {}", field(&response, "claim_key"));
        println!("  digest    : {}", field(&response, "plaintext_digest"));
        println!("  workspace  : {}", field(&response, "workspace_id"));
        if response["replaces"].is_object() {
            println!(
                "  replaces  : {} @ v{}",
                field(&response["replaces"], "artifact_id"),
                response["replaces"]["version"]
            );
        }
        if response["retirement"] == json!(true) {
            println!("  retirement: proposed; the claim retires when the proposal is approved");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `oml withdraw`: one `POST /v1/memories/{artifact_id}/withdraw`, the
/// author retracting its own held version (ADR-0083 d13).
fn run_withdraw(
    cli: &Cli,
    stored: &Credentials,
    args: &WithdrawArgs,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let mut body = json!({});
    if let Some(reason) = &args.reason {
        body["reason"] = json!(reason);
    }
    let path = format!("/v1/memories/{}/withdraw", segment(&args.artifact_id)?);
    let key = args
        .idempotency_key
        .clone()
        .unwrap_or_else(new_idempotency_key);
    let (_, response) = client.post_keyed(&path, &body, &key)?;
    if cli.json {
        print_json(&response);
    } else {
        println!(
            "withdrawn — {} (journal seq {})",
            field(&response, "artifact_id"),
            response["journal_seq"]
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// `oml read`, in `oml_read`'s three modes (ADR-0039 d4): with an artifact
/// id one `GET /v1/memories/{artifact_id}`, with `--query` one `POST
/// /v1/memories/recall` with the text, and with neither one `GET
/// /v1/memories`, the catalog unless `--own` (ADR-0092 d6). A flag outside
/// the mode its
/// arguments choose, `--query` beside an artifact id among them, is refused
/// by clap, naming it, before anything is sent.
fn run_read(cli: &Cli, stored: &Credentials, args: &ReadArgs) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let response = if let Some(id) = &args.artifact_id {
        client.get(&format!("/v1/memories/{}", segment(id)?))?.1
    } else if let Some(query) = &args.query {
        let mut body = json!({ "query": query, "include_text": true });
        if let Some(k) = args.k {
            body["k"] = json!(k);
        }
        if let Some(workspace) = &args.workspace {
            body["workspace"] = json!(workspace);
        }
        if !args.kinds.is_empty() {
            body["kinds"] = json!(args.kinds);
        }
        if args.include_unreviewed {
            body["include_unreviewed"] = json!(true);
        }
        if let Some(search) = &args.search {
            body["search"] = json!(search);
        }
        if let Some(mode) = &args.match_mode {
            body["match"] = json!(mode);
        }
        if let Some(repo) = &args.anchor_repo {
            body["anchor_repo"] = json!(repo);
        }
        if let Some(prefix) = &args.anchor_path_prefix {
            body["anchor_path_prefix"] = json!(prefix);
        }
        if !args.origin_kinds.is_empty() {
            body["origin_kinds"] = json!(args.origin_kinds);
        }
        // A mode is a string and anything else a list (ADR-0072 d5).
        if let Some(sources) = &args.sources {
            body["sources"] = match sources.as_str() {
                "scope" | "wide" => json!(sources),
                list => json!(list.split(',').map(str::trim).collect::<Vec<_>>()),
            };
        }
        let read = CallOpts {
            auth: true,
            idempotency: false, // read-shaped POST (http-api.md idempotency)
        };
        client.post("/v1/memories/recall", &body, &read)?.1
    } else {
        let limit = args.limit.map(|l| l.to_string());
        let kinds = (!args.kinds.is_empty()).then(|| args.kinds.join(","));
        let text = (args.text || args.own).then(|| "true".to_owned());
        let mut params = vec![("own", args.own.to_string())];
        for (name, value) in [
            ("workspace", args.workspace.as_ref()),
            ("state", args.state.as_ref()),
            ("kinds", kinds.as_ref()),
            ("prefix", args.prefix.as_ref()),
            ("include_text", text.as_ref()),
            ("since", args.since.as_ref()),
            ("until", args.until.as_ref()),
            ("limit", limit.as_ref()),
            ("cursor", args.cursor.as_ref()),
            ("anchor_repo", args.anchor_repo.as_ref()),
            ("anchor_path_prefix", args.anchor_path_prefix.as_ref()),
        ] {
            if let Some(value) = value {
                params.push((name, value.clone()));
            }
        }
        if !args.origin_kinds.is_empty() {
            params.push(("origin_kinds", args.origin_kinds.join(",")));
        }
        client
            .get(&format!("/v1/memories?{}", query_string(&params)))?
            .1
    };
    if cli.json {
        print_json(&response);
    } else if args.artifact_id.is_some() {
        println!(
            "{} @ v{}  {}  {}  {}",
            field(&response, "artifact_id"),
            response["version"],
            field(&response, "kind"),
            field(&response, "state"),
            field(&response, "workspace_path")
        );
        println!("  claim_key {}", field(&response, "claim_key"));
        println!("  | {}", field(&response, "text").replace('\n', "\n  | "));
    } else if args.query.is_some() {
        let hits = response["hits"].as_array().cloned().unwrap_or_default();
        println!(
            "{} hit(s) — query digest {}",
            hits.len(),
            field(&response, "query_digest")
        );
        for memory in response["steering"].as_array().into_iter().flatten() {
            println!();
            println!(
                "  steering  {}  {}",
                field(memory, "artifact_id"),
                field(memory, "claim_key")
            );
            println!("    | {}", field(memory, "text").replace('\n', "\n    | "));
        }
        for hit in &hits {
            let memory = &hit["memory"];
            println!();
            println!(
                "  {}  {}  ({})",
                field(memory, "artifact_id"),
                field(memory, "claim_key"),
                field(memory, "state")
            );
            println!("    | {}", field(memory, "text").replace('\n', "\n    | "));
        }
    } else {
        // The catalog's counts first, on its first page (ADR-0092 d8).
        if let Some(counts) = response["counts"].as_object() {
            let line: Vec<String> = counts
                .iter()
                .filter(|(_, n)| n.as_u64() != Some(0))
                .map(|(kind, n)| match n.get("at_least") {
                    Some(cap) => format!("{kind} {cap}+"),
                    None => format!("{kind} {n}"),
                })
                .collect();
            println!(
                "counts: {}",
                if line.is_empty() {
                    "none".to_owned()
                } else {
                    line.join(", ")
                }
            );
        }
        for item in response["items"].as_array().into_iter().flatten() {
            println!(
                "{}  {}  v{}  {}  {}  {}  {}",
                field(item, "artifact_id"),
                field(item, "claim_key"),
                item["version"],
                field(item, "state"),
                field(item, "kind"),
                field(item, "workspace_path"),
                field(item, "created_at")
            );
            if let Some(text) = item["text"].as_str() {
                println!("  | {}", text.replace('\n', "\n  | "));
            }
        }
        if let Some(cursor) = response["next_cursor"].as_str() {
            println!("more: --cursor {cursor}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `oml events`: one `GET /v2/events` page within the key's reader access,
/// each event digest-only (http-api.md events tail; ADR-0039 d10).
fn run_events(cli: &Cli, stored: &Credentials, args: &EventsArgs) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let mut params = Vec::new();
    if let Some(after) = args.after_seq {
        params.push(("after_seq", after.to_string()));
    }
    if let Some(workspace) = &args.workspace {
        params.push(("workspace", workspace.clone()));
    }
    if let Some(limit) = args.limit {
        params.push(("limit", limit.to_string()));
    }
    let path = if params.is_empty() {
        "/v2/events".to_owned()
    } else {
        format!("/v2/events?{}", query_string(&params))
    };
    let (_, page) = client.get(&path)?;
    if cli.json {
        print_json(&page);
    } else {
        // Served as stored, so `--json` stays verifiable; the lines read a
        // former event name as its successor (ADR-0091 d4).
        for event in page["items"].as_array().into_iter().flatten() {
            let event_type = oml_former_names::read_event_type(field(event, "event_type"))
                .map_err(|e| CliError::Local(e.to_string()))?;
            println!(
                "{}  {}  {}  {}",
                event["seq"],
                event_type,
                field(event, "at"),
                field(&event["payload"], "artifact_id")
            );
        }
        println!(
            "head seq {}; next --after-seq {}",
            page["head_seq"], page["next_after_seq"]
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// The arguments of `oml workspaces import`.
struct ImportArgs<'a> {
    file: &'a std::path::Path,
    into: &'a str,
    identity: &'a std::path::Path,
    expect_key: Option<&'a str>,
    expect_tenant: Option<&'a str>,
    dry_run: bool,
    allow_older: bool,
    upload: Option<&'a str>,
}

/// `oml workspaces import` (ADR-0084 d5, d7, d15, I10): pass 1 decrypts and
/// verifies the bundle here; pass 2 decrypts it again into the parts of one
/// upload session, unless `--upload` names a dry run's completed one; then
/// `POST /v2/imports` plans the import, or starts it and the CLI reads its
/// state until it ends. A session this run opened is abandoned on every
/// failure before the import takes it. No decrypted byte is written to
/// disk, and the identity is never printed and is zeroed after use.
fn run_workspaces_import(
    cli: &Cli,
    stored: &Credentials,
    args: &ImportArgs<'_>,
) -> Result<ExitCode, CliError> {
    use oml_cli::import::{LONG_TIMEOUT, POLL_EVERY, identity_of, refusal_hint, verify_pass};
    use zeroize::Zeroizing;
    #[cfg(unix)]
    if let Ok(meta) = std::fs::metadata(args.identity) {
        use std::os::unix::fs::PermissionsExt as _;
        if let Some(warning) = oml_cli::import::identity_mode_warning(meta.permissions().mode()) {
            eprintln!("{warning}");
        }
    }
    let identity_text = Zeroizing::new(
        std::fs::read_to_string(args.identity)
            .map_err(|e| CliError::local(format!("reading the identity file: {}", e.kind())))?,
    );
    let identity = Zeroizing::new(identity_of(&identity_text)?);
    drop(identity_text);
    let open = || {
        std::fs::File::open(args.file)
            .map(std::io::BufReader::new)
            .map_err(|e| CliError::local(format!("opening the bundle: {}", e.kind())))
    };
    let options = oml_bundle::VerifyOptions {
        expect_key: args.expect_key.map(str::to_owned),
        expect_tenant: args.expect_tenant.map(str::to_owned),
        ..oml_bundle::VerifyOptions::default()
    };
    // Pass 1: the CLI side of d7, before anything is sent.
    let verified =
        verify_pass(open()?, &identity, &options).map_err(|e| CliError::local(e.to_string()))?;
    let client = resolve(cli, stored)?;
    // R11: a target that does not exist yet holds no ledger, so this is a
    // first import, which pins the source's key and needs --expect-key.
    // An existing target may or may not hold one for this source, which only
    // the server reads, so it answers there.
    if args.expect_key.is_none()
        && !args.dry_run
        && let Err(CliError::Refused { status: 404, .. }) =
            client.get(&format!("/v2/workspaces/{}", segment(args.into)?))
    {
        return Err(CliError::local(format!(
            "{} does not exist yet, or this key cannot read it, so this is a first import from this source, which pins its key: check the source's key fingerprint out of band, then run again with --expect-key {} if it matches; nothing was uploaded",
            args.into, verified.signer_fingerprint
        )));
    }
    let slow = client.lasting(LONG_TIMEOUT);
    let (upload_id, opened) = match args.upload {
        Some(id) => (id.to_owned(), false),
        None => {
            let (_, session) = client.post(
                "/v2/uploads",
                &json!({ "purpose": "import", "size": verified.size, "sha256": verified.sha256 }),
                &CallOpts {
                    auth: true,
                    idempotency: false,
                },
            )?;
            let id = field(&session, "upload_id").to_owned();
            if let Err(e) = send_import_upload(&slow, &id, &identity, &open, &verified) {
                abandon_upload(&client, &id);
                return Err(e);
            }
            (id, true)
        }
    };
    drop(identity);
    let answer = slow.post(
        "/v2/imports",
        &json!({
            "upload_id": upload_id,
            "target": args.into,
            "dry_run": args.dry_run,
            "allow_older": args.allow_older,
            "expect_key": args.expect_key,
            "expect_tenant": args.expect_tenant,
        }),
        &CallOpts {
            auth: true,
            idempotency: false,
        },
    );
    if let Err(CliError::Refused {
        ref code,
        ref envelope,
        ..
    }) = answer
        && let Some(hint) = refusal_hint(
            code,
            envelope["error"]["details"]["field"].as_str(),
            &verified.signer_fingerprint,
        )
    {
        eprintln!("{hint}");
    }
    let mut response = match answer {
        Ok((_, response)) => response,
        Err(e) => {
            if opened {
                abandon_upload(&client, &upload_id);
            }
            return Err(e);
        }
    };
    if response.get("plan").is_none() {
        // I10: the import runs after its request answered; read it until it
        // ends.
        while response["import"]["state"] == "running" {
            std::thread::sleep(POLL_EVERY);
            let id = segment(field(&response["import"], "import_id"))?;
            response = client.get(&format!("/v2/imports/{id}"))?.1;
        }
    }
    let failed = response["import"]["state"] == "failed";
    if cli.json {
        print_json(&response);
    } else if let Some(plan) = response.get("plan") {
        println!(
            "plan (nothing written): {} from {} exported {}; signature {}",
            plan["counts"],
            plan["source"]["tenant_name"].as_str().unwrap_or(""),
            plan["exported_at"].as_str().unwrap_or(""),
            if plan["authenticated"].as_bool() == Some(true) {
                "authenticated against --expect-key"
            } else {
                "intact, not authenticated"
            },
        );
        println!(
            "upload {upload_id} kept: run again with --upload {upload_id} to import it without sending it again"
        );
    } else {
        let import = &response["import"];
        println!(
            "import {} {}: {} held for review; {} refused",
            field(import, "import_id"),
            field(import, "state"),
            import["counts"],
            import["refusals"].as_array().map_or(0, Vec::len),
        );
    }
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// Pass 2 into the session `upload_id`, then its completion: decrypts
/// again and sends each part as it is read, and refuses a bundle that
/// changed since pass 1.
fn send_import_upload<R: std::io::Read>(
    slow: &Client,
    upload_id: &str,
    identity: &str,
    open: &dyn Fn() -> Result<R, CliError>,
    verified: &oml_cli::import::Verified,
) -> Result<(), CliError> {
    use oml_cli::import::{PART_SIZE, upload_pass};
    let parts_path = format!("/v2/uploads/{}/parts", segment(upload_id)?);
    let (size, sha256) = upload_pass(open()?, identity, PART_SIZE, &mut |n, bytes| {
        slow.put_bytes(&format!("{parts_path}/{n}"), bytes)
            .map(|_| ())
    })?;
    if (size, sha256.as_str()) != (verified.size, verified.sha256.as_str()) {
        return Err(CliError::local(
            "the bundle changed between the two passes; nothing was imported",
        ));
    }
    slow.post(
        &format!("/v2/uploads/{}/complete", segment(upload_id)?),
        &json!({}),
        &CallOpts {
            auth: true,
            idempotency: false,
        },
    )?;
    Ok(())
}

/// Abandons the upload session this run opened (`DELETE /v2/uploads/{id}`),
/// so a failed import leaves no staged parts behind until the sweep. A
/// refused or unreachable abandon is named, never fatal: the sweep ends the
/// session at its expiry.
fn abandon_upload(client: &Client, upload_id: &str) {
    let abandoned = segment(upload_id).and_then(|id| client.delete(&format!("/v2/uploads/{id}")));
    let why = match abandoned {
        Ok(_) => return,
        Err(CliError::Refused { code, .. }) => code,
        Err(CliError::Local(m) | CliError::Unanswered(m)) => m,
    };
    eprintln!("the upload session {upload_id} was not abandoned ({why}); it expires on its own");
}

/// `oml workspaces status`: propose, ratify and the pending list over the
/// transition routes and the workspace read (ADR-0046 d3).
fn run_workspace_status(
    cli: &Cli,
    stored: &Credentials,
    command: &WorkspaceStatusCommand,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    match command {
        WorkspaceStatusCommand::Propose { workspace, to } => {
            let (_, response) = client.post(
                &format!("/v2/workspaces/{}/transitions", segment(workspace)?),
                &json!({ "to": to }),
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "proposed {} -> {} on {workspace}: transition {} waits for a second identity",
                    field(&response, "from"),
                    field(&response, "to"),
                    field(&response, "transition_id")
                );
            }
        }
        WorkspaceStatusCommand::Ratify {
            workspace,
            transition_id,
        } => {
            let (_, response) = client.post(
                &format!(
                    "/v2/workspaces/{}/transitions/{}/ratify",
                    segment(workspace)?,
                    segment(transition_id)?
                ),
                &json!({}),
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!("ratified: {workspace} is now {}", field(&response, "to"));
            }
        }
        WorkspaceStatusCommand::Pending { workspace } => {
            let (_, node) = client.get(&format!("/v2/workspaces/{}", segment(workspace)?))?;
            let pending = json!({
                "workspace_id": node["id"],
                "status": node["status"],
                "policy": node["policy"],
                "pending_transitions": node["pending_transitions"],
            });
            if cli.json {
                print_json(&pending);
            } else {
                println!("{workspace} is {}", field(&node, "status"));
                let rows = node["pending_transitions"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                if rows.is_empty() {
                    println!("    no proposal waits");
                }
                for t in &rows {
                    println!(
                        "    {}  {} -> {}",
                        field(t, "transition_id"),
                        field(t, "from"),
                        field(t, "to")
                    );
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `oml edges`: the `/v2/edges` family for kind claim and kind brief
/// (ADR-0046 d4, d7).
fn run_edges(
    cli: &Cli,
    stored: &Credentials,
    command: &EdgesCommand,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let response = match command {
        EdgesCommand::Propose { kind } => {
            let body = match kind {
                EdgeProposeCommand::Claim { from, to, label } => json!({
                    "kind": "claim", "from_artifact_id": from, "to_artifact_id": to, "label": label,
                }),
                EdgeProposeCommand::Brief {
                    artifact,
                    workspace,
                } => json!({
                    "kind": "brief", "from_artifact_id": artifact, "to_workspace_id": workspace,
                }),
            };
            client.post("/v2/edges", &body, &write_opts())?.1
        }
        EdgesCommand::Ratify { edge_id } => {
            client
                .post(
                    &format!("/v2/edges/{}/ratify", segment(edge_id)?),
                    &json!({}),
                    &write_opts(),
                )?
                .1
        }
        EdgesCommand::List {
            kind,
            workspace,
            artifact,
            limit,
            cursor,
        } => {
            let mut params = vec![("kind", kind.clone())];
            if let Some(workspace) = workspace {
                params.push(("workspace_id", workspace.clone()));
            }
            if let Some(artifact) = artifact {
                params.push(("artifact_id", artifact.clone()));
            }
            if let Some(limit) = limit {
                params.push(("limit", limit.to_string()));
            }
            if let Some(cursor) = cursor {
                params.push(("cursor", cursor.clone()));
            }
            client
                .get(&format!("/v2/edges?{}", query_string(&params)))?
                .1
        }
    };
    if cli.json {
        print_json(&response);
    } else {
        let rows = match response["items"].as_array() {
            Some(items) => items.clone(),
            None => vec![response.clone()],
        };
        for e in &rows {
            let to = e["to_artifact_id"]
                .as_str()
                .or_else(|| e["to_workspace_id"].as_str())
                .unwrap_or("?");
            println!(
                "{}  {}  {} -[{}]-> {}  {}",
                field(e, "edge_id"),
                field(e, "kind"),
                field(e, "from_artifact_id"),
                field(e, "label"),
                to,
                field(e, "state")
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `name=value` pairs joined by `&`, each value [`encode`]d.
fn query_string(params: &[(&str, String)]) -> String {
    params
        .iter()
        .map(|(name, value)| format!("{name}={}", encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// The review ceremony: `next` serves one held item, and `approve` and
/// `reject` record a verdict against the named serve or, with none, the one
/// item this key has claimed (ADR-0039 d10, call map D5).
fn run_review(
    cli: &Cli,
    stored: &Credentials,
    command: &ReviewCommand,
) -> Result<ExitCode, CliError> {
    match command {
        ReviewCommand::Next { needs } => run_review_next(cli, stored, needs.as_deref()),
        ReviewCommand::Fixes { confirm, revert } => {
            run_review_fixes(cli, stored, confirm.as_deref(), revert.as_deref())
        }
        ReviewCommand::List(args) => run_review_list(cli, stored, args),
        ReviewCommand::Approve {
            ids: Some(ids),
            note,
            ..
        } => run_bulk(cli, stored, "approve", ids, note.as_deref()),
        ReviewCommand::Reject {
            ids: Some(ids),
            reason,
            ..
        } => run_bulk(cli, stored, "reject", ids, Some(reason)),
        ReviewCommand::Approve { serve_id, note, .. } => {
            run_verdict(cli, stored, "approve", serve_id.as_deref(), note.as_deref())
        }
        ReviewCommand::Reject {
            serve_id, reason, ..
        } => run_verdict(cli, stored, "reject", serve_id.as_deref(), Some(reason)),
    }
}

/// `oml review next`: one `POST /v2/review/next`, which claims the item it
/// serves for this key; 204 when nothing in scope waits. `--needs` narrows
/// to the person lane or to the rest (ADR-0074 d19; ADR-0080 d16).
fn run_review_next(
    cli: &Cli,
    stored: &Credentials,
    needs: Option<&str>,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let body = needs.map_or_else(|| json!({}), |needs| json!({ "needs": needs }));
    let (status, response) = client.post("/v2/review/next", &body, &write_opts())?;
    if status == 204 {
        if cli.json {
            print_json(&json!({ "serve": Value::Null }));
        } else {
            println!("nothing in scope waits — the queue is empty (204).");
        }
        return Ok(ExitCode::SUCCESS);
    }
    if cli.json {
        print_json(&response);
    } else {
        let item = &response["item"];
        println!("serving 1 held item");
        println!();
        println!(
            "  artifact  {} @ v{}   kind: {}   author: {}",
            field(item, "artifact_id"),
            item["version"],
            field(item, "kind"),
            field(item, "principal")
        );
        println!("  claim_key {}", field(item, "claim_key"));
        println!("  source    {}", anchor_line(&item["anchor"]));
        println!("  valid     {}", valid_line(item));
        println!(
            "  body      | {}",
            field(item, "text").replace('\n', "\n            | ")
        );
        println!();
        println!(
            "  serve_id: {}  (expires {})",
            field(&response, "serve_id"),
            field(&response, "expires_at")
        );
        println!("  verdict with: oml review approve [--note …] | oml review reject --reason …");
    }
    Ok(ExitCode::SUCCESS)
}

/// `oml review list`: `GET /v2/review/held`, one page or with `--all`
/// every page, printed as a table, as ids one per line with `--ids-only`,
/// or as the pages' items with `--json` (ADR-0055 d11).
fn run_review_list(
    cli: &Cli,
    stored: &Credentials,
    args: &ReviewListArgs,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let mut query = vec![format!("limit={}", args.limit)];
    for (name, value) in [
        ("workspace", &args.workspace),
        ("author_key", &args.author_key),
        ("claim_key_prefix", &args.claim_key_prefix),
    ] {
        if let Some(value) = value {
            query.push(format!("{name}={}", encode(value)));
        }
    }
    let mut items: Vec<Value> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut q = query.clone();
        if let Some(c) = &cursor {
            q.push(format!("cursor={}", encode(c)));
        }
        let (_, page) = client.get(&format!("/v2/review/held?{}", q.join("&")))?;
        items.extend(page["items"].as_array().cloned().unwrap_or_default());
        match page["next_cursor"].as_str() {
            Some(next) if args.all => cursor = Some(next.to_owned()),
            _ => break,
        }
    }
    if cli.json {
        print_json(&json!({ "items": items }));
    } else if args.ids_only {
        for item in &items {
            println!("{}", field(item, "artifact_id"));
        }
    } else {
        println!("{} held item(s) you may judge", items.len());
        for item in &items {
            println!();
            println!(
                "  {}  {}  {}  from {}{}",
                field(item, "artifact_id"),
                field(item, "claim_key"),
                field(item, "workspace_path"),
                field(item, "principal"),
                if item["same_user"] == true {
                    " (you)"
                } else {
                    ""
                }
            );
            println!("    source {}", anchor_line(&item["anchor"]));
            println!("    valid  {}", valid_line(item));
            println!("    | {}", field(item, "text").replace('\n', "\n    | "));
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// A version's valid time as one line for a reviewer, and on a held
/// successor the end approving it gives the version it replaces, so a
/// verdict is given on what it makes true (proposed ADR-0059 d2, d4).
fn valid_line(item: &Value) -> String {
    let (from, until) = (item["valid_from"].as_str(), item["valid_until"].as_str());
    let mut line = if from.is_none() && until.is_none() {
        "none declared".to_owned()
    } else {
        format!(
            "from {}, {}",
            from.unwrap_or("any time"),
            until.map_or_else(|| "open".to_owned(), |u| format!("until {u}"))
        )
    };
    if let (Some(end), Some(version)) = (
        item["replaces_valid_until"].as_str(),
        item["replaces"]["version"].as_u64(),
    ) {
        line.push_str(&format!("; approving ends version {version} at {end}"));
    }
    line
}

/// An anchor as one line for a reviewer (ADR-0057 d2): the repository, the
/// commit's first 12 digits, the path and the fragment, or the page; `none`
/// for an unanchored version.
fn anchor_line(anchor: &Value) -> String {
    if !anchor.is_object() {
        return "none".to_owned();
    }
    let fragment = anchor["fragment"]
        .as_str()
        .map_or_else(String::new, |f| format!("#{f}"));
    match anchor["repo"].as_str() {
        Some(repo) => {
            let rev = anchor["rev"].as_str().unwrap_or("");
            // The rev is the server's text: twelve characters, never a
            // slice at byte 12, which may fall inside one.
            format!(
                "{repo} @ {} {}{fragment}",
                rev.chars().take(12).collect::<String>(),
                anchor["path"].as_str().unwrap_or("")
            )
        }
        None => format!("{}{fragment}", anchor["uri"].as_str().unwrap_or("")),
    }
}

/// The ids `--ids` names: a file of one per line, `-` for standard input,
/// or a comma-separated list; blank lines and `#` comments are skipped.
fn bulk_ids(spec: &str) -> Result<Vec<String>, CliError> {
    let text = if spec == "-" {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
            .map_err(|e| CliError::local(format!("reading ids from standard input: {e}")))?;
        buf
    } else if std::path::Path::new(spec).is_file() {
        std::fs::read_to_string(spec)
            .map_err(|e| CliError::local(format!("reading ids from {spec}: {e}")))?
    } else {
        spec.replace(',', "\n")
    };
    let ids: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToOwned::to_owned)
        .collect();
    if ids.is_empty() {
        return Err(CliError::local("--ids names no id"));
    }
    Ok(ids)
}

/// `oml review approve --ids` and `oml review reject --ids`: the ids in
/// requests of 200 to `POST /v2/review/verdicts/bulk`, each under an
/// idempotency key derived from the whole list, the verdict, the reason
/// and the request's position, so a rerun of the same command replays;
/// one line per item and a summary; exit 1 when any item was skipped
/// (ADR-0055 d11, d12).
fn run_bulk(
    cli: &Cli,
    stored: &Credentials,
    verdict: &str,
    spec: &str,
    reason: Option<&str>,
) -> Result<ExitCode, CliError> {
    use sha2::Digest as _;
    let client = resolve(cli, stored)?;
    let ids = bulk_ids(spec)?;
    let digest = sha2::Sha256::digest(
        format!("{verdict}\n{}\n{}", reason.unwrap_or(""), ids.join("\n")).as_bytes(),
    );
    let run: String = digest[..12].iter().map(|b| format!("{b:02x}")).collect();
    let mut all = Vec::new();
    let mut counts = [0_u64; 3];
    for (n, chunk) in ids.chunks(200).enumerate() {
        let mut body = json!({ "verdict": verdict, "artifact_ids": chunk });
        if let Some(reason) = reason {
            body["reason"] = json!(reason);
        }
        let (_, answer) = client.post_keyed(
            "/v2/review/verdicts/bulk",
            &body,
            &format!("bulk-{run}-{n}"),
        )?;
        for (i, key) in ["approved", "rejected", "skipped"].iter().enumerate() {
            counts[i] += answer["summary"][key].as_u64().unwrap_or(0);
        }
        all.extend(answer["items"].as_array().cloned().unwrap_or_default());
    }
    if cli.json {
        print_json(&json!({
            "items": all,
            "summary": { "approved": counts[0], "rejected": counts[1], "skipped": counts[2] },
        }));
    } else {
        for item in &all {
            match item["code"].as_str() {
                Some(code) => println!(
                    "  {}  skipped  {code}: {}",
                    field(item, "artifact_id"),
                    field(item, "message")
                ),
                None => println!(
                    "  {}  {}  journal seq {}{}{}",
                    field(item, "artifact_id"),
                    field(item, "outcome"),
                    item["journal_seq"],
                    if item["same_user"] == true {
                        "  same user"
                    } else {
                        ""
                    },
                    if item["replayed"] == true {
                        "  replayed"
                    } else {
                        ""
                    }
                ),
            }
        }
        println!(
            "{} approved, {} rejected, {} skipped",
            counts[0], counts[1], counts[2]
        );
    }
    Ok(if counts[2] == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// One `POST /v2/review/verdicts`: `serve_id` only when named, so the server
/// applies an unnamed verdict to the key's one claimed item and refuses it
/// naming `serve_id` when the key holds none or several; a note or reason
/// travels as `reason` (ADR-0039 d10).
fn run_verdict(
    cli: &Cli,
    stored: &Credentials,
    verdict: &str,
    serve_id: Option<&str>,
    reason: Option<&str>,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let mut body = json!({ "verdict": verdict });
    if let Some(serve_id) = serve_id {
        body["serve_id"] = json!(serve_id);
    }
    if let Some(reason) = reason {
        body["reason"] = json!(reason);
    }
    let (_, response) = client.post("/v2/review/verdicts", &body, &write_opts())?;
    if cli.json {
        print_json(&response);
    } else {
        println!(
            "verdict recorded — {} is {} (journal seq {})",
            field(&response, "artifact_id"),
            field(&response, "state"),
            response["journal_seq"]
        );
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------------------
// oml keys issue-agent (ADR-0028 d1)
// ---------------------------------------------------------------------------

/// One `POST /v2/keys` for a contributor key named `<key-name>`, with
/// submitter and reader grants on the workspace, or with `--role submitter`
/// a submitter key with the submitter grant alone (ADR-0093 d6), that workspace as its default and the
/// `--crew` when one is given, each reference as the operator spelled it,
/// then two lines on stdout: `export OML_URL=…` and `export
/// OML_API_KEY=…`, the second reading the value back from the `--out` file
/// when one is named, and with `--out` a third, the `claude plugin configure
/// --values-stdin` line that reads the same file (ADR-0040 d4). Every other
/// line goes to stderr, and no key value reaches either stream once `--out`
/// is given (cli/reference.md `oml keys issue-agent`; ADR-0028 d1; ADR-0026
/// d12). With `--connect-code` the body carries `connect_code: true` and the
/// answer is printed as [`connect_code_printed`] declares (ADR-0031 d7).
fn run_issue_agent(
    cli: &Cli,
    stored: &Credentials,
    args: &IssueAgentArgs,
) -> Result<ExitCode, CliError> {
    if args.connect_code && args.out.is_some() {
        return Err(CliError::local(
            "refused: --out cannot be used with --connect-code because connector keys have no \
             value to write; omit --out. Nothing was sent",
        ));
    }
    if cli.json && args.out.is_some() {
        return Err(CliError::local(
            "refused: --out cannot be used with --json because the JSON response contains the \
             key value; omit one flag. Nothing was sent",
        ));
    }
    // With --user the workspace defaults to the user's root, which the
    // server resolves in that user's namespace (ADR-0044 d11).
    let workspace = match (&args.workspace, &args.user) {
        (Some(workspace), _) => workspace.clone(),
        (None, Some(user)) => user.clone(),
        (None, None) => {
            return Err(CliError::local(
                "refused: --workspace is required without --user; nothing was sent",
            ));
        }
    };
    let client = resolve(cli, stored)?;
    let key_file = match &args.out {
        Some(path) => Some(KeyFile::create(path).map_err(CliError::Local)?),
        None => None,
    };
    // A reader key reads the workspace and writes nothing (#528).
    let grants = if args.role == "reader" {
        vec![(workspace.clone(), "reader".to_owned())]
    } else {
        with_reader_beside_submitters(&[(workspace.clone(), "submitter".to_owned())], &args.role)
    };
    let mut body = json!({
        "role": args.role,
        "name": args.key_name,
        "used_by": args.used_by,
        "principal": args.principal,
        "default_workspace": workspace,
        "grants": grants_json(&grants),
    });
    if !args.create_workspaces.is_empty() {
        body["create_workspaces"] = json!(new_workspaces_json(&args.create_workspaces));
    }
    if let Some(crew) = &args.crew {
        body["crew"] = json!(crew);
    }
    if let Some(user) = &args.user {
        body["user"] = json!(user);
    }
    if args.connect_code {
        body["connect_code"] = json!(true);
    }
    if let Some(scope) = &args.guardrail_scope {
        body["guardrail_scope"] = json!(scope);
    }
    args.limits.add_to(&mut body);
    let (response, replayed) = match client.post_replayable("/v2/keys", &body, &write_opts()) {
        Ok(answer) => answer,
        Err(failure) => {
            // The empty file holds nothing either way, so it goes.
            if let Some(Err(e)) = key_file.map(KeyFile::discard) {
                eprintln!("note: {e}");
            }
            return Err(match failure {
                // Sent, and no usable answer: a key may exist whose value
                // nothing kept. Only a refusal, or a failure before sending,
                // means no key exists (ADR-0028 d1).
                CliError::Unanswered(why) if args.connect_code => CliError::Unanswered(format!(
                    "{why}. Connector key {name} for principal {principal} may have been issued, \
                     but its connect code is lost. Run `oml keys show {name}` to check, then \
                     `oml keys connect-code {name}` to mint a new code",
                    name = args.key_name,
                    principal = args.principal,
                )),
                CliError::Unanswered(why) => CliError::Unanswered(format!(
                    "{why}. Key {name} for principal {principal} may have been issued, but its \
                     value is lost. Run `oml keys show {name}` to check. If it exists, revoke it, \
                     or until revocation is available run `oml deny set {name} universe`; issue \
                     a replacement under a new name",
                    name = args.key_name,
                    principal = args.principal,
                )),
                refused_or_unsent => refused_or_unsent,
            });
        }
    };
    if args.connect_code {
        let printed = connect_code_printed(&response, replayed, cli.json);
        for line in &printed.stdout {
            println!("{line}");
        }
        for line in &printed.stderr {
            eprintln!("{line}");
        }
        return Ok(ExitCode::from(printed.exit));
    }
    if cli.json {
        print_json(&response);
        return Ok(ExitCode::SUCCESS);
    }
    let key_id = field(&response, "key_id");
    let key_name = field(&response, "name");
    let lost = |why: &str| {
        CliError::local(format!(
            "key {key_name} ({key_id}) was issued, but {why}; its value is lost and the key is \
             live. Revoke it, or until revocation is available run `oml deny set {key_id} universe`"
        ))
    };
    let value = response["value"]
        .as_str()
        .ok_or_else(|| lost("the 201 carried no value"))?;
    let (key_line, configure) = match key_file {
        None => (format!("export OML_API_KEY={}", sh_quote(value)), None),
        Some(file) => {
            let shown = file.shown().to_owned();
            file.write_value(value)
                .map_err(|e| lost(&format!("writing {shown} failed ({e})")))?;
            (
                format!("export OML_API_KEY=\"$(cat {})\"", sh_quote(&shown)),
                Some([
                    configure_line(client.server(), &shown),
                    setup_line(client.server(), &shown),
                ]),
            )
        }
    };
    println!("export OML_URL={}", sh_quote(client.server()));
    println!("{key_line}");
    for line in configure.iter().flatten() {
        println!("{line}");
    }

    let granted: Vec<String> = response["grants"]
        .as_array()
        .map(|grants| {
            grants
                .iter()
                .map(|g| {
                    format!(
                        "{}:{}{}{}",
                        field(g, "workspace_node_id"),
                        field(g, "role"),
                        if g["covers"] == "node" { ":node" } else { "" },
                        if g["create"] == true { ":create" } else { "" }
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    eprintln!(
        "key {key_name} ({key_id}) issued to {} ({}, used by {}): {}; default workspace {}",
        field(&response, "principal"),
        args.role,
        args.used_by,
        granted.join(", "),
        field(&response, "default_workspace_id"),
    );
    // The limits the key was issued with (ADR-0079 d7); stdout stays the
    // lines a shell evaluates.
    for line in key_limit_lines(&response) {
        eprintln!("    {line}");
    }
    match &args.out {
        Some(_) => eprintln!(
            "the value is only in the key file (0600); delete it once `claude plugin \
             configure` has succeeded and no shell still reads it"
        ),
        None => eprintln!("the value is shown once, above, and never again"),
    }
    Ok(ExitCode::SUCCESS)
}

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

// ---------------------------------------------------------------------------
// oml workspaces tree (ADR-0026 d15)
// ---------------------------------------------------------------------------

/// The tree from `root` down, by id or name, the default `universe`
/// (ADR-0026 d6): every page of `GET /v2/workspaces/{workspace_id}/tree`, in
/// the server's path order. Under an admin key the read answers by the key
/// role, for every workspace of the tenant and structure only; under a reader
/// key it answers within reader effective access, every node outside it
/// absent (http-api.md workspaces; ADR-0026 d15). `--json` prints the pages'
/// items as one page (cli/reference.md `oml workspaces`).
fn run_workspaces_tree(cli: &Cli, stored: &Credentials, root: &str) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let tree = format!("/v2/workspaces/{}/tree", segment(root)?);
    let mut nodes: Vec<Value> = Vec::new();
    let mut path = tree.clone();
    loop {
        let (_, page) = client.get(&path)?;
        nodes.extend(page["items"].as_array().into_iter().flatten().cloned());
        match page["next_cursor"].as_str() {
            Some(cursor) => path = format!("{tree}?cursor={}", encode(cursor)),
            None => break,
        }
    }
    if cli.json {
        print_json(&json!({ "items": nodes, "next_cursor": Value::Null }));
    } else {
        for node in &nodes {
            println!(
                "{}  {}  {}  crew: {}",
                field(node, "path"),
                field(node, "id"),
                field(node, "status"),
                crew_line(&node["crew"])
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------------------
// oml crews (ADR-0026 d8)
// ---------------------------------------------------------------------------

/// The crew verbs, admin key role throughout, each crew by id or name, sent
/// as typed (cli/reference.md `oml crews`; http-api.md crews; ADR-0026 d4,
/// d8). `oml crews list` reads every page, and `--json` prints their items as
/// one page.
fn run_crews(
    cli: &Cli,
    stored: &Credentials,
    command: &CrewsCommand,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let (verb, response) = match command {
        CrewsCommand::Create {
            name,
            charter_uri,
            external_manager,
        } => {
            let mut body = json!({ "name": name });
            if let Some(uri) = charter_uri {
                body["charter_uri"] = json!(uri);
            }
            if let Some(manager) = external_manager {
                body["external_manager"] = json!(manager);
            }
            (
                "created ",
                client.post("/v2/crews", &body, &write_opts())?.1,
            )
        }
        CrewsCommand::Update(args) => {
            let mut body = json!({});
            if let Some(uri) = &args.charter_uri {
                body["charter_uri"] = json!(uri);
            }
            if let Some(manager) = &args.external_manager {
                body["external_manager"] = json!(manager);
            }
            let path = format!("/v2/crews/{}/update", segment(&args.crew)?);
            ("updated ", client.post(&path, &body, &write_opts())?.1)
        }
        CrewsCommand::Show { crew } => {
            ("", client.get(&format!("/v2/crews/{}", segment(crew)?))?.1)
        }
        CrewsCommand::List => {
            let mut items: Vec<Value> = Vec::new();
            let mut path = "/v2/crews".to_owned();
            loop {
                let (_, page) = client.get(&path)?;
                items.extend(page["items"].as_array().into_iter().flatten().cloned());
                match page["next_cursor"].as_str() {
                    Some(cursor) => path = format!("/v2/crews?cursor={}", encode(cursor)),
                    None => break,
                }
            }
            ("", json!({ "items": items, "next_cursor": Value::Null }))
        }
    };
    if cli.json {
        print_json(&response);
    } else if let CrewsCommand::List = command {
        for crew in response["items"].as_array().into_iter().flatten() {
            println!(
                "{}  charter: {}  external manager: {}",
                crew_line(crew),
                field(crew, "charter_uri"),
                field(crew, "external_manager")
            );
        }
    } else {
        println!("{verb}crew {}", crew_line(&response));
        println!("    charter uri     : {}", field(&response, "charter_uri"));
        println!(
            "    external manager: {}",
            field(&response, "external_manager")
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// `oml admin read` (cli/reference.md `oml admin`; ADR-0045 d21): one
/// memory by id, or every page of a search, each answer journalled by the
/// server as `memory.admin_read`.
/// `oml admin rescan`: every window of the journal, summed by rule, each
/// finding listed by artifact, version, state and place (ADR-0065 d13), and,
/// with `record_held`, each window's held versions' findings recorded under
/// the server's rule set (d15; ADR-0074 d28). Exits 1 when any stored
/// version would be refused, so a script notices.
fn run_rescan(
    cli: &Cli,
    client: &Client,
    record_held: bool,
    rates: bool,
) -> Result<ExitCode, CliError> {
    let mut after = 0_u64;
    let mut scanned = 0_u64;
    let mut by_origin: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut recorded = 0_u64;
    let mut version;
    let mut findings: Vec<Value> = Vec::new();
    let mut by_rule: std::collections::BTreeMap<(String, String, String), u64> =
        std::collections::BTreeMap::new();
    loop {
        let path = format!("/v2/admin/rescan?after_seq={after}");
        let (_, page) = if record_held {
            client.post(&path, &json!({}), &write_opts())?
        } else {
            client.get(&path)?
        };
        version = page["scanner_version"].clone();
        scanned += page["scanned"].as_u64().unwrap_or(0);
        recorded += page["recorded"].as_u64().unwrap_or(0);
        for (origin, n) in page["scanned_by_origin"].as_object().into_iter().flatten() {
            *by_origin.entry(origin.clone()).or_default() += n.as_u64().unwrap_or(0);
        }
        findings.extend(page["findings"].as_array().cloned().unwrap_or_default());
        for r in page["by_rule"].as_array().into_iter().flatten() {
            *by_rule
                .entry((
                    field(r, "class").to_owned(),
                    field(r, "rule").to_owned(),
                    field(r, "action").to_owned(),
                ))
                .or_default() += r["count"].as_u64().unwrap_or(0);
        }
        match page["next_after_seq"].as_u64() {
            Some(next) if next > after => after = next,
            _ => break,
        }
    }
    let vetoes = findings.iter().filter(|f| f["action"] == "veto").count();
    if rates {
        let report = rescan_rates(version.as_str(), scanned, &by_origin, &findings);
        if cli.json {
            print_json(&report);
        } else {
            for line in rates_lines(&report) {
                println!("{line}");
            }
        }
        return Ok(ExitCode::SUCCESS);
    }
    if cli.json {
        print_json(&json!({
            "scanner_version": version,
            "scanned": scanned,
            "by_rule": by_rule.iter().map(|((class, rule, action), count)| json!({
                "class": class, "rule": rule, "action": action, "count": count,
            })).collect::<Vec<_>>(),
            "findings": findings,
            "recorded": recorded,
        }));
    } else {
        println!(
            "rescan      : {scanned} stored version(s) under {}, {} finding(s), {recorded} held version(s) recorded",
            version.as_str().unwrap_or("no pinned rule set"),
            findings.len()
        );
        for ((class, rule, action), count) in &by_rule {
            println!("  {action:<4} {class}: {rule} ×{count}");
        }
        for f in &findings {
            println!(
                "  {}  v{}  {}  {}  {}: {} at {}",
                field(f, "artifact_id"),
                f["version"],
                field(f, "state"),
                field(f, "action"),
                field(f, "class"),
                field(f, "rule"),
                field(f, "at")
            );
        }
        if vetoes > 0 {
            println!(
                "next        : {vetoes} finding(s) the scan would refuse today; nothing was refused or deleted. Read each with oml admin read <artifact_id> and retire or demote it"
            );
        }
    }
    Ok(if vetoes > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// The smallest count the rates print: a cell below it prints `<5` and no
/// rate, so a glance at the table does not show a single version. It is not
/// disclosure control: an origin's exact `scanned` and the difference
/// between a total and its other cells can recover a hidden count, so the
/// output is the operator's (ADR-0074 d18).
const RATES_FLOOR: usize = 5;

/// A count as the rates print it: the number at or above the floor, `<5`
/// below it, and its share of `of` only when printed.
fn floored(n: usize, of: u64) -> (Value, Value) {
    if n == 0 || n >= RATES_FLOOR {
        #[allow(clippy::cast_precision_loss)]
        let rate = if of == 0 { 0.0 } else { n as f64 / of as f64 };
        (json!(n), json!(rate))
    } else {
        (json!(format!("<{RATES_FLOOR}")), Value::Null)
    }
}

/// The dry run's aggregates (ADR-0074 d18): for each class, each origin
/// kind and each class within an origin, the versions with a finding the
/// scan would refuse or hold, and their share of the versions scanned (of
/// that origin, for an origin). A version counts once per cell however
/// many findings it carries. Every count from 1 to 4 prints as `<5` with
/// no rate. The output is for the operator, not for publication: the floor
/// can be undone by subtraction and by a small origin's exact `scanned`.
/// The rescan answer the CLI reads carries artifact ids, as the plain
/// rescan does for the admin key that asks; no id, claim key or text leaves
/// this function.
fn rescan_rates(
    version: Option<&str>,
    scanned: u64,
    by_origin: &std::collections::BTreeMap<String, u64>,
    findings: &[Value],
) -> Value {
    use std::collections::{BTreeMap, BTreeSet};
    let verdict = |f: &Value| {
        if f["action"] == "veto" {
            "refuse"
        } else {
            "hold"
        }
    };
    let mut by_class: BTreeMap<(String, &str), BTreeSet<String>> = BTreeMap::new();
    let mut origin_hits: BTreeMap<(String, &str), BTreeSet<String>> = BTreeMap::new();
    let mut cells: BTreeMap<(String, String, &str), BTreeSet<String>> = BTreeMap::new();
    for f in findings {
        let id = format!("{}#{}", field(f, "artifact_id"), f["version"]);
        let class = field(f, "class").to_owned();
        let origin = f["origin"].as_str().unwrap_or("none").to_owned();
        by_class
            .entry((class.clone(), verdict(f)))
            .or_default()
            .insert(id.clone());
        origin_hits
            .entry((origin.clone(), verdict(f)))
            .or_default()
            .insert(id.clone());
        cells
            .entry((class, origin, verdict(f)))
            .or_default()
            .insert(id);
    }
    json!({
        "scanner_version": version,
        "scanned": scanned,
        "floor": RATES_FLOOR,
        "classes": by_class.iter().map(|((class, v), ids)| {
            let (n, rate) = floored(ids.len(), scanned);
            json!({ "class": class, "verdict": v, "versions": n, "rate": rate })
        }).collect::<Vec<_>>(),
        "origins": by_origin.iter().map(|(origin, n)| {
            let count = |v: &str| origin_hits.get(&(origin.clone(), v)).map_or(0, BTreeSet::len);
            let (refuse, refuse_rate) = floored(count("refuse"), *n);
            let (hold, hold_rate) = floored(count("hold"), *n);
            json!({
                "origin": origin, "scanned": n,
                "refuse": refuse, "refuse_rate": refuse_rate,
                "hold": hold, "hold_rate": hold_rate,
            })
        }).collect::<Vec<_>>(),
        "cells": cells.iter().map(|((class, origin, v), ids)| {
            let of = by_origin.get(origin).copied().unwrap_or(0);
            let (n, rate) = floored(ids.len(), of);
            json!({ "class": class, "origin": origin, "verdict": v, "versions": n, "rate": rate })
        }).collect::<Vec<_>>(),
    })
}

/// The text form of [`rescan_rates`]: percentages to two places, a dash
/// for a cell under the floor.
fn rates_lines(report: &Value) -> Vec<String> {
    let pct = |v: &Value| {
        v.as_f64()
            .map_or_else(|| "-".to_owned(), |r| format!("{:.2}%", 100.0 * r))
    };
    let count = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_owned);
    let mut out = vec![format!(
        "rates       : {} stored version(s) under {}; aggregates for the operator, not for publication; counts under {} print as <{}",
        report["scanned"],
        report["scanner_version"]
            .as_str()
            .unwrap_or("no pinned rule set"),
        RATES_FLOOR,
        RATES_FLOOR
    )];
    for c in report["classes"].as_array().into_iter().flatten() {
        out.push(format!(
            "  class  {:<28} {:<6} {:>6} version(s)  {}",
            field(c, "class"),
            field(c, "verdict"),
            count(&c["versions"]),
            pct(&c["rate"])
        ));
    }
    for o in report["origins"].as_array().into_iter().flatten() {
        out.push(format!(
            "  origin {:<8} {:>6} scanned  refuse {} ({})  hold {} ({})",
            field(o, "origin"),
            o["scanned"],
            count(&o["refuse"]),
            pct(&o["refuse_rate"]),
            count(&o["hold"]),
            pct(&o["hold_rate"])
        ));
    }
    for c in report["cells"].as_array().into_iter().flatten() {
        out.push(format!(
            "  cell   {:<28} {:<8} {:<6} {:>6} version(s)  {}",
            field(c, "class"),
            field(c, "origin"),
            field(c, "verdict"),
            count(&c["versions"]),
            pct(&c["rate"])
        ));
    }
    out
}

/// `oml admin investigate`: opens an investigation with `reason`, closes
/// `close`, or lists the key's open ones (ADR-0089 d10, d15).
/// One line per fix: its id, act, state, the version it serves, whether it
/// loosens a guardrail, its age, the acts this key may take, and its reason.
/// One admin act as `oml admin acts` prints it (ADR-0097 d2): its id,
/// act, class, state, the acting person and its reason.
fn admin_act_line(a: &Value) -> String {
    format!(
        "{}  {}  {}  {}{}  by {}  {}",
        field(a, "act_id"),
        field(a, "act"),
        field(a, "class"),
        field(a, "state"),
        if a["sole_admin"] == true {
            " (sole_admin)"
        } else {
            ""
        },
        field(a, "actor_person"),
        field(a, "reason"),
    )
}

fn print_fixes(cli: &Cli, answer: &Value) -> Result<ExitCode, CliError> {
    if cli.json {
        print_json(answer);
        return Ok(ExitCode::SUCCESS);
    }
    let line = |f: &Value| {
        let acts = f["acts"]
            .as_array()
            .map(|a| {
                let named: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
                format!(
                    "  acts {}",
                    if named.is_empty() {
                        "none (another person confirms it)".to_owned()
                    } else {
                        named.join(",")
                    }
                )
            })
            .unwrap_or_default();
        format!(
            "{}  {}  {}  serves {}  loosens_guardrail {}  age {}s{}  {}",
            field(f, "fix_id"),
            field(f, "act"),
            field(f, "state"),
            field(f, "served_artifact_id"),
            f["loosens_guardrail"],
            f["age_seconds"],
            acts,
            field(f, "reason").replace('\n', " ")
        )
    };
    match answer["items"].as_array() {
        Some(items) => items.iter().for_each(|f| println!("{}", line(f))),
        None => println!("{}", line(answer)),
    }
    Ok(ExitCode::SUCCESS)
}

/// `oml review fixes`: the unconfirmed admin fixes this person's reviewer
/// key sees, or with --confirm or --revert one settled (ADR-0089 d8, d15).
fn run_review_fixes(
    cli: &Cli,
    stored: &Credentials,
    confirm: Option<&str>,
    revert: Option<&str>,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let write = CallOpts {
        auth: true,
        idempotency: false,
    };
    let answer = match (confirm, revert) {
        (Some(id), _) => {
            client
                .post(
                    &format!("/v2/review/fixes/{}/confirm", segment(id)?),
                    &json!({}),
                    &write,
                )?
                .1
        }
        (None, Some(id)) => {
            client
                .post(
                    &format!("/v2/review/fixes/{}/revert", segment(id)?),
                    &json!({}),
                    &write,
                )?
                .1
        }
        (None, None) => client.get("/v2/review/fixes")?.1,
    };
    print_fixes(cli, &answer)
}

fn run_investigate(
    cli: &Cli,
    client: &Client,
    reason: Option<&str>,
    close: Option<&str>,
) -> Result<ExitCode, CliError> {
    let write = CallOpts {
        auth: true,
        idempotency: false,
    };
    let answer = match (reason, close) {
        (Some(reason), _) => {
            client
                .post(
                    "/v2/admin/investigations",
                    &json!({ "reason": reason }),
                    &write,
                )?
                .1
        }
        (None, Some(id)) => {
            client
                .post(
                    &format!("/v2/admin/investigations/{}/close", segment(id)?),
                    &json!({}),
                    &write,
                )?
                .1
        }
        (None, None) => client.get("/v2/admin/investigations?state=open")?.1,
    };
    if cli.json {
        print_json(&answer);
        return Ok(ExitCode::SUCCESS);
    }
    let line = |i: &Value| {
        format!(
            "{}  {}  reads {}  age {}s  {}",
            field(i, "investigation_id"),
            field(i, "state"),
            i["reads"],
            i["age_seconds"],
            field(i, "reason").replace('\n', " ")
        )
    };
    match answer["items"].as_array() {
        Some(items) => items.iter().for_each(|i| println!("{}", line(i))),
        None => println!("{}", line(&answer)),
    }
    Ok(ExitCode::SUCCESS)
}

fn run_admin(
    cli: &Cli,
    stored: &Credentials,
    command: &AdminCommand,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let (memory, query, workspace, state, investigation) = match command {
        AdminCommand::Rescan { record_held, rates } => {
            return run_rescan(cli, &client, *record_held, *rates);
        }
        AdminCommand::Investigate { reason, close } => {
            return run_investigate(cli, &client, reason.as_deref(), close.as_deref());
        }
        AdminCommand::Fix {
            act,
            target,
            reason,
            body,
            title,
        } => {
            let mut sent = json!({ "act": act, "target": target, "reason": reason });
            if let Some(body) = body {
                sent["body"] = json!(if body == "-" {
                    let mut text = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
                        .map_err(|e| CliError::Local(format!("--body -: {e}")))?;
                    text
                } else {
                    body.clone()
                });
            }
            if let Some(title) = title {
                sent["title"] = json!(title);
            }
            let write = CallOpts {
                auth: true,
                idempotency: false,
            };
            let fix = client.post("/v2/admin/fixes", &sent, &write)?.1;
            return print_fixes(cli, &fix);
        }
        AdminCommand::Acts { state } => {
            let path = match state {
                Some(state) => format!("/v2/admin/acts?state={}", segment(state)?),
                None => "/v2/admin/acts".to_owned(),
            };
            let acts = client.get(&path)?.1;
            if cli.json {
                print_json(&acts);
            } else {
                for a in acts["items"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                {
                    println!("{}", admin_act_line(a));
                }
            }
            return Ok(ExitCode::SUCCESS);
        }
        AdminCommand::Confirm { act } => {
            let path = format!("/v2/admin/acts/{}/confirm", segment(act)?);
            let settled = client.post(&path, &json!({}), &write_opts())?.1;
            if cli.json {
                print_json(&settled);
            } else {
                println!("{}", admin_act_line(&settled));
            }
            return Ok(ExitCode::SUCCESS);
        }
        AdminCommand::Decline { act, reason } => {
            let path = format!("/v2/admin/acts/{}/decline", segment(act)?);
            let body = match reason {
                Some(reason) => json!({ "reason": reason }),
                None => json!({}),
            };
            let settled = client.post(&path, &body, &write_opts())?.1;
            if cli.json {
                print_json(&settled);
            } else {
                println!("{}", admin_act_line(&settled));
            }
            return Ok(ExitCode::SUCCESS);
        }
        AdminCommand::Settle {
            act,
            confirm,
            reverse: _,
        } => {
            let path = format!("/v2/admin/acts/{}/settle", segment(act)?);
            let body = json!({ "outcome": if *confirm { "confirm" } else { "reverse" } });
            let settled = client.post(&path, &body, &write_opts())?.1;
            if cli.json {
                print_json(&settled);
            } else {
                println!("{}", admin_act_line(&settled));
            }
            return Ok(ExitCode::SUCCESS);
        }
        AdminCommand::Fixes { all } => {
            let state = if *all { "all" } else { "unconfirmed" };
            let fixes = client.get(&format!("/v2/admin/fixes?state={state}"))?.1;
            return print_fixes(cli, &fixes);
        }
        AdminCommand::Read {
            memory,
            query,
            workspace,
            state,
            investigation,
        } => (memory, query, workspace, state, investigation),
    };
    let line = |m: &Value| {
        format!(
            "{}  {}  {}  {}  {}",
            field(m, "artifact_id"),
            field(m, "state"),
            field(m, "workspace_path"),
            field(m, "key"),
            field(m, "text").replace('\n', " ")
        )
    };
    if let Some(memory) = memory {
        let (_, read) = client.get(&format!(
            "/v2/admin/memories/{}?investigation={}",
            segment(memory)?,
            segment(investigation)?
        ))?;
        if cli.json {
            print_json(&read);
        } else {
            println!("{}", line(&read));
            println!("    principal   : {}", field(&read, "principal"));
            println!("    user        : {}", field(&read, "user"));
            println!("    derives from: {}", read["derives_from"]);
            println!("    source      : {}", anchor_line(&read["anchor"]));
            println!("    replaces    : {}", field(&read, "replaces"));
            for v in read["verdicts"].as_array().into_iter().flatten() {
                println!(
                    "    verdict     : {} by {} at {}",
                    field(v, "verdict"),
                    field(v, "key"),
                    field(v, "at")
                );
            }
        }
        return Ok(ExitCode::SUCCESS);
    }
    let mut body = json!({ "investigation": investigation });
    if let Some(q) = query {
        body["query"] = json!(q);
    }
    if let Some(w) = workspace {
        body["workspace"] = json!(w);
    }
    if let Some(s) = state {
        body["state"] = json!(s);
    }
    let mut items: Vec<Value> = Vec::new();
    loop {
        let read = CallOpts {
            auth: true,
            idempotency: false, // read-shaped POST (http-api.md idempotency)
        };
        let (_, page) = client.post("/v2/admin/memories/search", &body, &read)?;
        items.extend(page["items"].as_array().into_iter().flatten().cloned());
        match page["next_cursor"].as_str() {
            Some(cursor) => body["cursor"] = json!(cursor),
            None => break,
        }
    }
    if cli.json {
        print_json(&json!({ "items": items, "next_cursor": Value::Null }));
    } else {
        for m in &items {
            println!("{}", line(m));
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The allowlist verbs, admin key role throughout (cli/reference.md `oml
/// web`; http-api.md web pages; ADR-0045 d2). The email travels in a body,
/// never a path; `oml web list` reads every page.
/// `oml search show` and `oml search set-default` (ADR-0049 d4).
fn run_search(
    cli: &Cli,
    stored: &Credentials,
    command: &SearchCommand,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let settings = match command {
        SearchCommand::Show => client.get("/v2/search")?.1,
        SearchCommand::Federation {
            enabled,
            default_sources,
            max_sources,
            reason,
        } => {
            let policy = if enabled.is_none() && default_sources.is_none() && max_sources.is_none()
            {
                client.get("/v2/federation")?.1
            } else {
                let mut body = serde_json::Map::new();
                if let Some(on) = enabled {
                    body.insert("enabled".to_owned(), Value::from(on == "on"));
                }
                if let Some(d) = default_sources {
                    body.insert("default_sources".to_owned(), Value::from(d.as_str()));
                }
                if let Some(m) = max_sources {
                    body.insert("max_sources".to_owned(), Value::from(*m));
                }
                if let Some(reason) = reason {
                    body.insert("reason".to_owned(), Value::from(reason.as_str()));
                }
                let (status, response) =
                    client.post("/v2/federation", &Value::Object(body), &write_opts())?;
                if status == 202 {
                    if cli.json {
                        print_json(&response);
                    } else {
                        println!("{}", federation_pending("the federation policy", &response));
                    }
                    return Ok(ExitCode::SUCCESS);
                }
                response
            };
            if cli.json {
                print_json(&policy);
            } else {
                println!(
                    "federation: {}",
                    if policy["enabled"] == true {
                        "on"
                    } else {
                        "off"
                    }
                );
                println!("default sources: {}", field(&policy, "default_sources"));
                println!("max sources: {}", field(&policy, "max_sources"));
            }
            return Ok(ExitCode::SUCCESS);
        }
        SearchCommand::SetDefault { pipeline } => {
            let name = (pipeline != "deployment").then_some(pipeline.as_str());
            client
                .post(
                    "/v2/search/default",
                    &json!({ "pipeline": name }),
                    &write_opts(),
                )?
                .1
        }
    };
    if cli.json {
        print_json(&settings);
    } else {
        let enabled: Vec<&str> = settings["enabled"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        // The deployment's default runs while the tenant's is not served
        // here (ADR-0049 d15).
        if settings["tenant_default_served"] == false {
            println!(
                "effective: {} (the tenant's default is not served here)",
                field(&settings, "effective")
            );
        } else {
            println!("effective: {}", field(&settings, "effective"));
        }
        println!(
            "tenant default: {}",
            settings["tenant_default"]
                .as_str()
                .unwrap_or("(the deployment's)")
        );
        println!(
            "deployment default: {}",
            field(&settings, "deployment_default")
        );
        println!("enabled: {}", enabled.join(", "));
    }
    Ok(ExitCode::SUCCESS)
}

fn run_usage(
    cli: &Cli,
    stored: &Credentials,
    command: &UsageCommand,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let body = match command {
        UsageCommand::Settings { query_text: None } => client.get("/v2/usage/settings")?.1,
        UsageCommand::Settings {
            query_text: Some(on),
        } => {
            client
                .post(
                    "/v2/usage/settings",
                    &json!({ "query_text": on == "on" }),
                    &write_opts(),
                )?
                .1
        }
        UsageCommand::Events { limit } => client.get(&format!("/v2/usage/events?limit={limit}"))?.1,
        UsageCommand::Eval { pipelines } => {
            let read = CallOpts {
                auth: true,
                idempotency: false, // read-shaped POST (http-api.md idempotency)
            };
            client
                .post("/v2/usage/eval", &json!({ "pipelines": pipelines }), &read)?
                .1
        }
    };
    if cli.json {
        print_json(&body);
        return Ok(ExitCode::SUCCESS);
    }
    match command {
        UsageCommand::Settings { .. } => {
            println!(
                "recording: {}",
                if body["recording"] == true {
                    "on"
                } else {
                    "off"
                }
            );
            match body["query_text_since"].as_str() {
                Some(since) => println!("query text: on since {since}"),
                None => println!("query text: off (digests only)"),
            }
            println!(
                "retention: {} days",
                body["retention_days"].as_u64().unwrap_or(30)
            );
        }
        UsageCommand::Events { .. } => {
            for e in body["items"].as_array().into_iter().flatten() {
                let labels: Vec<&str> = e["labels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|l| l["label"].as_str())
                    .collect();
                println!(
                    "{}  {}  {}  {} hits  {}  {}",
                    field(e, "created_at"),
                    field(e, "recall_id"),
                    field(e, "pipeline"),
                    e["hits"].as_array().map_or(0, Vec::len),
                    if labels.is_empty() {
                        "-".to_owned()
                    } else {
                        labels.join(",")
                    },
                    e["query_text"].as_str().unwrap_or("(no text)")
                );
            }
        }
        UsageCommand::Eval { .. } => {
            println!(
                "labelled: {} in {} days, {} skipped (key revoked)",
                body["labelled"].as_u64().unwrap_or(0),
                body["window_days"].as_u64().unwrap_or(30),
                body["skipped_revoked"].as_u64().unwrap_or(0)
            );
            for p in body["pipelines"].as_array().into_iter().flatten() {
                let n = |k: &str| p[k].as_f64().unwrap_or(0.0);
                println!(
                    "{}: {} queries  success@1 {:.3}  success@5 {:.3}  nDCG@10 {:.3}  R@10 {:.3}",
                    field(p, "pipeline"),
                    p["queries"].as_u64().unwrap_or(0),
                    n("success_at_1"),
                    n("success_at_5"),
                    n("ndcg_at_10"),
                    n("recall_at_10"),
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_web(cli: &Cli, stored: &Credentials, command: &WebCommand) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let entry_line = |entry: &Value| {
        format!(
            "{}  user: {}{}",
            field(entry, "email"),
            field(entry, "user"),
            if entry["admin"].as_bool() == Some(true) {
                "  admin"
            } else {
                ""
            }
        )
    };
    match command {
        WebCommand::Allow {
            email,
            user,
            admin,
            reason,
        } => {
            let mut body = json!({ "email": email, "user": user, "admin": admin });
            if let Some(reason) = reason {
                body["reason"] = json!(reason);
            }
            let (_, entry) = client.post("/v2/web/emails", &body, &write_opts())?;
            if cli.json {
                print_json(&entry);
            } else {
                println!("allowed {}", entry_line(&entry));
            }
        }
        WebCommand::Remove { email, reason } => {
            let mut body = json!({ "email": email });
            if let Some(reason) = reason {
                body["reason"] = json!(reason);
            }
            let (_, result) = client.post("/v2/web/emails/remove", &body, &write_opts())?;
            if cli.json {
                print_json(&result);
            } else {
                println!("removed {email}; the user's agent keys keep working");
            }
        }
        WebCommand::List => {
            let mut items: Vec<Value> = Vec::new();
            let mut path = "/v2/web/emails".to_owned();
            loop {
                let (_, page) = client.get(&path)?;
                items.extend(page["items"].as_array().into_iter().flatten().cloned());
                match page["next_cursor"].as_str() {
                    Some(cursor) => path = format!("/v2/web/emails?cursor={}", encode(cursor)),
                    None => break,
                }
            }
            if cli.json {
                print_json(&json!({ "items": items, "next_cursor": Value::Null }));
            } else {
                for entry in &items {
                    println!("{}", entry_line(entry));
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------------------
// oml audit verify-chain
// ---------------------------------------------------------------------------

/// Live verification reads the chain export (`kind: chain`), the one
/// surface that serves the whole per-tenant chain: the events tail is a
/// filtered feed whose omitted events and nulled identities break
/// recomputation (http-api.md events tail; ADR-0025 d15). This server serves
/// no chain export yet, so after the local checks the verb refuses by name
/// instead of reporting breaks it did not find.
fn run_verify_chain(
    cli: &Cli,
    stored: &Credentials,
    args: &VerifyChainArgs,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    if args.signing_keys.is_empty() {
        return Err(CliError::local(
            "the verifier holds an authenticated key set, confirmed out of band — the chain \
             cannot vouch for its own key: pass --signing-key <64-hex raw ed25519 public key> \
             (chain-format.md, \"How the public key reaches an export recipient\")",
        ));
    }
    for key in &args.signing_keys {
        parse_verifying_key(key).map_err(CliError::Local)?;
    }
    // The signed bytes include the tenant id; the key's own is derived
    // through whoami, as the credentials file holds only the pair
    // (chain-format.md signed fields; ADR-0039 d15).
    let tenant = match &args.tenant {
        Some(tenant) => tenant.clone(),
        None => {
            let me = client.whoami()?;
            me["tenant"]["id"]
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| CliError::local("whoami names no tenant: pass --tenant"))?
        }
    };
    let mode = if args.genesis { " from genesis" } else { "" };
    Err(CliError::local(format!(
        "live verification of tenant {tenant}{mode} reads the chain export (kind: chain), which \
         this server does not serve yet; the events tail is a filtered feed, not a verification \
         surface (http-api.md events tail)"
    )))
}

// ---------------------------------------------------------------------------
// oml login
// ---------------------------------------------------------------------------

/// `oml login <url>` (ADR-0039 d15): the key read without echo from the
/// terminal, or from stdin when stdin is not one; `GET /v2/whoami` under it;
/// and only on success `url` and `api_key`, nothing else, written to the
/// owner-only credentials file. A refusal writes nothing and exits 1 with
/// the envelope's code; the key is never printed.
fn run_login(cli: &Cli, target: &str) -> Result<ExitCode, CliError> {
    let key = read_key(target)?;
    let client = Client::new(target.to_owned(), Some(key.clone()));
    let me = client.whoami()?;
    let path = creds::store(&Credentials {
        url: Some(client.server().to_owned()),
        api_key: Some(key),
    })
    .map_err(CliError::Local)?;
    if cli.json {
        print_json(&me);
    } else {
        println!(
            "logged in to {} as key {} ({}): role {}, tenant {}, user root {}, default workspace {}",
            client.server(),
            field(&me, "name"),
            field(&me, "key_id"),
            field(&me, "role"),
            field(&me["tenant"], "name"),
            user_root_line(&me),
            field(&me["default_workspace"], "path"),
        );
        println!(
            "url and api_key stored in {} (owner-only); the flags and OML_URL with \
             OML_API_KEY still win over it",
            path.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// A key's user root as `oml login` and `oml doctor` print it:
/// `universe.<user>`, or `none (tenant key: …)` (ADR-0044 d11, d14).
fn user_root_line(me: &Value) -> String {
    match me["user"]["name"].as_str() {
        Some(user) => format!("universe.{user}"),
        None => "none (tenant key: names resolve in the shared namespace)".to_owned(),
    }
}

/// The API key for `url`, read as [`read_secret`] reads it.
fn read_key(url: &str) -> Result<String, CliError> {
    read_secret(&format!("API key for {url}"))
}

/// The secret `what` names: prompted for on the terminal and read without
/// echo when stdin is one, else the first line of stdin, its line ending
/// cut, so it never reaches argv. An empty secret, or one with a byte
/// outside printable ASCII, which no key or enrollment token holds, is
/// refused before anything is sent.
fn read_secret(what: &str) -> Result<String, CliError> {
    use std::io::{BufRead as _, IsTerminal as _};
    let stdin = std::io::stdin();
    let (line, from) = if stdin.is_terminal() {
        let line = rpassword::prompt_password(format!("{what}: ")).map_err(|e| {
            CliError::local(format!("cannot read the {what} from the terminal: {e}"))
        })?;
        (line, "the terminal")
    } else {
        let mut line = String::new();
        stdin
            .lock()
            .read_line(&mut line)
            .map_err(|e| CliError::local(format!("cannot read the {what} from stdin: {e}")))?;
        (line, "stdin")
    };
    let secret = line.trim();
    if secret.is_empty() {
        return Err(CliError::local(format!(
            "no {what} on {from}; nothing was sent and nothing written"
        )));
    }
    if !secret.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(CliError::local(format!(
            "the {what} on {from} holds a character outside printable ASCII; nothing was sent \
             and nothing written"
        )));
    }
    Ok(secret.to_owned())
}

// ---------------------------------------------------------------------------
// oml quickstart
// ---------------------------------------------------------------------------

/// Quote one argument for the POSIX shell command printed to the operator.
fn shell_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// `oml quickstart` (ADR-0041 d9): refuse a directory a quickstart wrote
/// into and a machine without `docker compose`, before anything is written;
/// then the compose file and its `.env`, `docker compose up --detach
/// --wait`, the claim of the token the lead logged, the crew workspace, an
/// agent key and a reviewer key in owner-only files, a write held, approved
/// and recalled, and the lines that hand the agent key on. No key value is
/// printed.
fn run_quickstart(args: &QuickstartArgs) -> Result<ExitCode, CliError> {
    use oml_cli::quickstart;
    let dir = std::path::absolute(&args.dir)
        .map_err(|e| CliError::local(format!("cannot resolve {}: {e}", args.dir.display())))?;
    let shown = dir.display().to_string();
    let shell_dir = shell_word(&shown);
    if let Some(name) = quickstart::existing(&dir) {
        return Err(CliError::local(format!(
            "refused: quickstart.exists — {shown} already contains {name} from a quickstart. \
             Use another --dir, or remove the stack and its data with `docker compose \
             --project-directory {shell_dir} down -v`. Nothing was changed"
        )));
    }
    let project = quickstart::project_name(&dir);
    let compose = |rest: &[&str]| {
        let mut command = std::process::Command::new("docker");
        command
            .arg("compose")
            .arg("--project-name")
            .arg(&project)
            .arg("--project-directory")
            .arg(&dir)
            .args(rest)
            .stdin(std::process::Stdio::null());
        command
    };
    match std::process::Command::new("docker")
        .args(["compose", "version"])
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(out) if out.status.success() => {}
        _ => {
            return Err(CliError::local(
                "refused: quickstart.docker — `docker compose version` failed. Install Docker \
                 with Compose v2; nothing was written",
            ));
        }
    }

    std::fs::create_dir_all(&dir)
        .map_err(|e| CliError::local(format!("cannot create {shown}: {e}")))?;
    let version = env!("CARGO_PKG_VERSION");
    std::fs::write(
        dir.join("compose.yaml"),
        quickstart::compose(&project, version, args.port),
    )
    .map_err(|e| CliError::local(format!("cannot write {shown}/compose.yaml: {e}")))?;
    write_owner_only(&dir.join(".env"), &quickstart::env_file())?;
    let keys = dir.join("keys");
    owner_only_dir(&keys)?;

    eprintln!(
        "starting the stack: docker compose --project-directory {shell_dir} up --detach --wait"
    );
    let status = compose(&["up", "--detach", "--wait"])
        .stdout(std::process::Stdio::from(std::io::stderr()))
        .status()
        .map_err(|e| CliError::local(format!("cannot run docker compose up: {e}")))?;
    if !status.success() {
        return Err(CliError::local(format!(
            "docker compose up failed ({status}); `docker compose --project-directory {shell_dir} \
             logs` shows why"
        )));
    }
    let url = format!("http://127.0.0.1:{}", args.port);
    wait_ready(&url)?;
    let logs = compose(&["logs", "oml-server"])
        .output()
        .map_err(|e| CliError::local(format!("cannot run docker compose logs: {e}")))?;
    let logs = String::from_utf8_lossy(&logs.stdout);
    let token = quickstart::enrollment_token(&logs).ok_or_else(|| {
        CliError::local(format!(
            "no enrollment token in the lead's log: a lead prints one on its first start \
             against an empty database only; `docker compose --project-directory {shell_dir} logs \
             oml-server` shows what it printed"
        ))
    })?;

    let admin = Child {
        dir: &dir,
        pair: None,
    };
    admin.json(
        &[
            "--url",
            &url,
            "bootstrap",
            "claim",
            "quickstart-admin",
            "--token-stdin",
            "--tenant-name",
            "quickstart",
            "--principal",
            "platform/admin",
            "--person",
            &args.person,
            "--email",
            &args.email,
        ],
        Some(token),
    )?;
    admin.json(&["workspaces", "create", "universe", "crew"], None)?;
    let agent_file = keys.join("agent");
    let agent_shown = agent_file.display().to_string();
    admin.text(&[
        "keys",
        "issue-agent",
        "quickstart-agent",
        "--principal",
        "crew/agent",
        "--workspace",
        "crew",
        "--out",
        &agent_shown,
    ])?;
    let reviewer_file =
        oml_cli::issue::KeyFile::create(&keys.join("reviewer")).map_err(CliError::Local)?;
    let reviewer_shown = reviewer_file.shown().to_owned();
    let issued = admin
        .json(
            &[
                "keys",
                "issue",
                "quickstart-reviewer",
                "--role",
                "reviewer",
                "--used-by",
                "person",
                "--principal",
                "crew/reviewer",
                "--default-workspace",
                "crew",
                "--grant",
                "crew:reviewer",
            ],
            None,
        )
        .and_then(|reviewer| {
            reviewer["value"]
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    CliError::Unanswered("the reviewer key's issuance carried no value".to_owned())
                })
        });
    let reviewer_key = match issued {
        Ok(key) => key,
        Err(e) => {
            // The empty file would only mislead; the refusal is the issuance's.
            let _ = reviewer_file.discard();
            return Err(e);
        }
    };
    reviewer_file
        .write_value(&reviewer_key)
        .map_err(CliError::Local)?;
    let agent_key = std::fs::read_to_string(&agent_file)
        .map_err(|e| CliError::local(format!("cannot read {agent_shown}: {e}")))?;
    let agent_key = agent_key.trim_end().to_owned();

    // The self-check: written as the agent, held, approved by a different
    // identity, recalled.
    let agent = Child {
        dir: &dir,
        pair: Some((&url, &agent_key)),
    };
    let reviewer = Child {
        dir: &dir,
        pair: Some((&url, &reviewer_key)),
    };
    let held = agent.json(
        &[
            "write",
            "The quickstart wrote this memory as the agent, and a reviewer approved it.",
            "--idempotency-key",
            "quickstart/1",
        ],
        None,
    )?;
    let check = |ok: bool, what: &str, answer: &Value| {
        if ok {
            Ok(())
        } else {
            Err(CliError::local(format!(
                "the self-check failed: {what}: {answer}"
            )))
        }
    };
    check(held["state"] == "held", "the write was not held", &held)?;
    reviewer.json(&["review", "next"], None)?;
    let approved = reviewer.json(&["review", "approve"], None)?;
    check(
        approved["state"] == "promoted",
        "the approval did not promote",
        &approved,
    )?;
    let recalled = agent.json(&["read", "--query", "quickstart wrote"], None)?;
    let found = recalled["hits"].as_array().is_some_and(|hits| {
        hits.iter()
            .any(|h| h["memory"]["artifact_id"] == held["artifact_id"])
    });
    check(found, "the approved memory was not recalled", &recalled)?;

    println!("OML {version} is running at {url}, from {shown}/compose.yaml.");
    println!(
        "The self-check passed: a write by the agent key was held, approved by the reviewer \
         key, and recalled."
    );
    println!();
    println!("Keys, each in an owner-only file and printed nowhere:");
    println!("  agent    (writes and recalls in crew)  {agent_shown}");
    println!("  reviewer (a person approves writes)    {reviewer_shown}");
    println!("  admin    (url and api_key)             {shown}/credentials");
    println!();
    println!("Give Claude Code on this machine the agent key:");
    println!("  oml setup claude-code --url {url} --key-file {agent_shown}");
    println!("Or store the agent key for the CLI, pasting it when asked:");
    println!("  oml login {url}");
    println!("Review what agents write as the reviewer:");
    println!(
        "  OML_URL={url} OML_API_KEY=\"$(cat {})\" oml review next",
        sh_quote(&reviewer_shown)
    );
    println!("Run admin verbs against this stack:");
    println!(
        "  OML_CLI_CREDENTIALS={} oml keys issue-agent …",
        sh_quote(&format!("{shown}/credentials"))
    );
    Ok(ExitCode::SUCCESS)
}

/// One run of this same `oml` binary under the quickstart's own credentials
/// file, `<dir>/credentials`, and with `pair` as `OML_URL` and
/// `OML_API_KEY`; the caller's own pair and file are never read.
struct Child<'a> {
    dir: &'a std::path::Path,
    pair: Option<(&'a str, &'a str)>,
}

impl Child<'_> {
    fn command(&self, args: &[&str]) -> Result<std::process::Command, CliError> {
        let exe = std::env::current_exe()
            .map_err(|e| CliError::local(format!("cannot find the oml binary: {e}")))?;
        let mut command = std::process::Command::new(exe);
        command
            .env("OML_CLI_CREDENTIALS", self.dir.join("credentials"))
            .env_remove("OML_URL")
            .env_remove("OML_API_KEY")
            .env_remove("OML_CLI_SERVER")
            .env_remove("OML_CLI_KEY");
        if let Some((url, key)) = self.pair {
            command.env("OML_URL", url).env("OML_API_KEY", key);
        }
        command.args(args);
        Ok(command)
    }

    /// The verb's output, or its refusal with what it printed, which holds
    /// no key: a refusal envelope or a local reason.
    fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<std::process::Output, CliError> {
        use std::io::Write as _;
        use std::process::Stdio;
        let verb = args
            .iter()
            .find(|a| !a.starts_with('-') && !a.contains("://"))
            .map_or("oml", |v| *v);
        let mut child = self
            .command(args)?
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CliError::local(format!("cannot run oml {verb}: {e}")))?;
        if let Some(mut pipe) = child.stdin.take() {
            let text = stdin.map(|s| format!("{s}\n")).unwrap_or_default();
            pipe.write_all(text.as_bytes())
                .map_err(|e| CliError::local(format!("cannot write to oml {verb}: {e}")))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| CliError::local(format!("oml {verb} did not finish: {e}")))?;
        if out.status.success() {
            Ok(out)
        } else {
            let said = |b: &[u8]| String::from_utf8_lossy(b).trim().to_owned();
            Err(CliError::local(format!(
                "the quickstart's `oml {}` failed ({}): {} {}",
                args.iter()
                    .take_while(|a| !a.starts_with("--"))
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" "),
                out.status,
                said(&out.stdout),
                said(&out.stderr),
            )))
        }
    }

    /// The verb run with `--json`: its one document.
    fn json(&self, args: &[&str], stdin: Option<&str>) -> Result<Value, CliError> {
        let mut with = vec!["--json"];
        with.extend_from_slice(args);
        let out = self.run(&with, stdin)?;
        serde_json::from_slice(&out.stdout).map_err(|e| {
            CliError::Unanswered(format!("oml {} printed no JSON document: {e}", args[0]))
        })
    }

    /// The verb run for its effect, its output dropped.
    fn text(&self, args: &[&str]) -> Result<(), CliError> {
        self.run(args, None).map(drop)
    }
}

/// Polls `url`'s `/readyz` for up to two minutes, the image's start
/// included.
fn wait_ready(url: &str) -> Result<(), CliError> {
    let probe = Client::with_timeout(url.to_owned(), None, std::time::Duration::from_secs(2));
    for _ in 0..120 {
        if matches!(probe.probe("/readyz"), Ok((200, _))) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    Err(CliError::local(format!(
        "no ready lead at {url}/readyz after two minutes"
    )))
}

/// Writes `text` to the new file `path`, owner-only from its creation.
fn write_owner_only(path: &std::path::Path, text: &str) -> Result<(), CliError> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    oml_cli::owner_only::open_options(&mut options);
    let shown = path.display();
    let mut file = options
        .open(path)
        .map_err(|e| CliError::local(format!("cannot create {shown}: {e}")))?;
    oml_cli::owner_only::restrict(&file)
        .and_then(|()| file.write_all(text.as_bytes()))
        .and_then(|()| file.sync_all())
        .map_err(|e| CliError::local(format!("cannot write {shown}: {e}")))
}

/// Creates the directory `path`, owner-only (0700 on Unix).
fn owner_only_dir(path: &std::path::Path) -> Result<(), CliError> {
    // Only Unix narrows the mode, so the binding is mutated on Unix alone.
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(path)
        .map_err(|e| CliError::local(format!("cannot create {}: {e}", path.display())))
}

// ---------------------------------------------------------------------------
// oml setup claude-code
// ---------------------------------------------------------------------------

/// The pair `oml setup claude-code` hands the plugin: `--key-file`'s key
/// with the `--url` flag, or the pair as [`resolve`] takes it (ADR-0039 d15,
/// ADR-0041 d7 step 1). A key named without a URL, by any form, asks for the
/// server on a terminal and is refused as `setup.no_server` off one, before
/// anything is sent; a key file is never paired with `OML_URL`, and a URL
/// named without a key is refused (ADR-0076 d4).
fn setup_pair(
    cli: &Cli,
    stored: &Credentials,
    key_file: Option<&std::path::Path>,
) -> Result<(Client, String), CliError> {
    let Some(path) = key_file else {
        return match first_source(cli, stored) {
            Some(Source {
                url,
                key: Some(key),
                ..
            }) => {
                let url = match url {
                    Some(url) => url.clone(),
                    None => ask_for_the_server(false)?,
                };
                Ok((Client::new(url, Some(key.clone())), key.clone()))
            }
            Some(source) => Err(missing(&source.half())),
            None => Err(missing(NO_PAIR)),
        };
    };
    if cli.start.flag_key.is_some() {
        return Err(CliError::local(
            "refused: --key-file with --api-key, which each name the key; nothing was sent",
        ));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| CliError::local(format!("cannot read {}: {e}", path.display())))?;
    let key = text.trim_end_matches(['\n', '\r']);
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(CliError::local(format!(
            "refused: {} holds no key on one line of printable ASCII; nothing was sent",
            path.display()
        )));
    }
    let url = match &cli.start.flag_url {
        Some(url) => url.clone(),
        None => ask_for_the_server(cli.start.env_url.is_some())?,
    };
    Ok((Client::new(url, Some(key.to_owned())), key.to_owned()))
}

/// The server for a key named without a URL: asked for on stderr and read
/// from stdin when both are terminals, the test `oml login` uses for its key
/// ([`read_secret`]); refused as `setup.no_server` otherwise, or on an empty
/// answer. `oml_url_set` adds why `OML_URL` was not used (ADR-0076 d4).
fn ask_for_the_server(oml_url_set: bool) -> Result<String, CliError> {
    use oml_cli::setup;
    use std::io::{BufRead as _, IsTerminal as _, Write as _};
    let refused = || CliError::local(setup::no_server(oml_url_set));
    if !(std::io::stdin().is_terminal() && std::io::stderr().is_terminal()) {
        return Err(refused());
    }
    if oml_url_set {
        eprintln!("{}", setup::OML_URL_NOT_PAIRED);
    }
    eprint!("{}", setup::URL_PROMPT);
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).map_err(|e| {
        CliError::local(format!("cannot read the server URL from the terminal: {e}"))
    })?;
    setup::answered_url(&line).ok_or_else(refused)
}

/// `oml setup claude-code` (ADR-0041 d7): check the key through whoami and
/// refuse an admin or operator key, then install the plugin at this CLI's
/// own tag and save the pair through `claude plugin configure
/// --values-stdin`, or print the commands with `--print` or with no `claude`
/// on `PATH`. The key goes to the configure command's stdin alone.
fn run_setup_claude_code(
    cli: &Cli,
    stored: &Credentials,
    args: &SetupClaudeCodeArgs,
) -> Result<ExitCode, CliError> {
    use oml_cli::setup;
    let (client, key) = setup_pair(cli, stored, args.key_file.as_deref())?;
    let me = client.whoami()?;
    let role = me["role"].as_str().unwrap_or_default();
    if setup::refuses_role(role) {
        return Err(CliError::local(format!(
            "refused: setup.admin_key — key {} has role {role}; the agent plugin requires an \
             agent key, not an admin or operator key. Issue one with \
             `oml keys issue-agent --out`; nothing was installed",
            field(&me, "name"),
        )));
    }
    let version = env!("CARGO_PKG_VERSION");
    let url = client.server();
    let print = |why: &str| {
        println!("{why}; run these to install and configure the plugin:");
        let shown = args.key_file.as_deref().map(|p| {
            std::path::absolute(p)
                .unwrap_or_else(|_| p.to_owned())
                .display()
                .to_string()
        });
        for line in setup::printed(version, url, shown.as_deref()) {
            println!("  {line}");
        }
        if shown.is_none() {
            println!("with OML_API_KEY set to the key in the shell that runs the last line");
        }
        println!(
            "if `claude plugin marketplace list` already shows {} at another tag, run \
             `claude plugin marketplace remove {}` first: Claude Code refuses an add whose \
             source differs, and the remove uninstalls its plugins",
            setup::MARKETPLACE_NAME,
            setup::MARKETPLACE_NAME,
        );
        Ok(ExitCode::SUCCESS)
    };
    if args.print {
        return print("--print");
    }
    let claude_version = match std::process::Command::new("claude")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
    {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return print("no claude on PATH");
        }
        Err(e) => {
            return Err(CliError::local(format!(
                "cannot run `claude --version`: {e}"
            )));
        }
        Ok(out) => String::from_utf8_lossy(&out.stdout).into_owned(),
    };
    let min = setup::dotted(setup::MIN_CLAUDE_CODE);
    match setup::claude_code_version(&claude_version) {
        Some(found) if found >= setup::MIN_CLAUDE_CODE => {}
        found => {
            return Err(CliError::local(format!(
                "refused: setup.claude_code_version — `claude --version` reported {}; Claude \
                 Code {min} or later is required for `claude plugin configure --values-stdin`. \
                 Upgrade Claude Code; nothing was installed",
                found.map_or_else(|| format!("{:?}", claude_version.trim()), setup::dotted),
            )));
        }
    }
    let refused = |why: String| {
        CliError::local(format!(
            "refused: setup.marketplace — {why}; nothing was changed"
        ))
    };
    let listed = claude_output(&setup::marketplace_list_command())?;
    let mut plugins = vec![setup::PLUGIN.to_owned()];
    match setup::marketplace(&listed, version).ok_or_else(|| {
        refused("`claude plugin marketplace list --json` printed no JSON array".to_owned())
    })? {
        setup::Marketplace::Pinned => {}
        setup::Marketplace::Absent => run_claude(&setup::add_command(version), None)?,
        setup::Marketplace::Foreign(source) => {
            return Err(refused(format!(
                "Claude Code already holds a marketplace named {} from {source}, not {};                  `claude plugin marketplace remove {}` removes it and the plugins installed                  from it",
                setup::MARKETPLACE_NAME,
                setup::MARKETPLACE,
                setup::MARKETPLACE_NAME,
            )));
        }
        setup::Marketplace::Repin(from) => {
            let installed = claude_output(&setup::plugin_list_command())?;
            plugins.extend(setup::other_plugins(&installed).ok_or_else(|| {
                refused("`claude plugin list --json` printed no JSON array".to_owned())
            })?);
            eprintln!(
                "moving marketplace {} from {from} to {}#v{version}: removing it uninstalls \
                 {}, which are installed again",
                setup::MARKETPLACE_NAME,
                setup::MARKETPLACE,
                plugins.join(", "),
            );
            run_claude(&setup::remove_command(), None)?;
            run_claude(&setup::add_command(version), None)?;
        }
    }
    for plugin in &plugins {
        run_claude(&setup::install_command(plugin), None)?;
    }
    run_claude(&setup::configure_command(), Some(&setup::values(url, &key)))?;
    if cli.json {
        print_json(&json!({
            "plugin": setup::PLUGIN,
            "version": version,
            "url": url,
            "key": me["name"],
            "role": role,
        }));
    } else {
        println!(
            "{} installed from {}#v{version} and configured for {url} as key {} (role {role}); \
             run /reload-plugins in a running Claude Code session, or restart it",
            setup::PLUGIN,
            setup::MARKETPLACE,
            field(&me, "name"),
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Runs `claude <args>` with no stdin and returns its stdout; a failure
/// names the command.
fn claude_output(args: &[String]) -> Result<String, CliError> {
    let shown = format!("claude {}", args.join(" "));
    let out = std::process::Command::new("claude")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| CliError::local(format!("cannot run `{shown}`: {e}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(CliError::local(format!(
            "`{shown}` failed ({}); nothing was changed",
            out.status
        )))
    }
}

/// Runs `claude <args>`, writing `stdin` to it when given, else none; a
/// failure names the command, which never holds the key.
fn run_claude(args: &[String], stdin: Option<&str>) -> Result<(), CliError> {
    use std::io::Write as _;
    use std::process::{Command as Process, Stdio};
    let shown = format!("claude {}", args.join(" "));
    let mut child = Process::new("claude")
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        // Its own output goes to stderr, so --json's stdout stays one
        // document.
        .stdout(Stdio::from(std::io::stderr()))
        .spawn()
        .map_err(|e| CliError::local(format!("cannot run `{shown}`: {e}")))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes())
            .and_then(|()| pipe.write_all(b"\n"))
            .map_err(|e| CliError::local(format!("cannot write to `{shown}`: {e}")))?;
    }
    let status = child
        .wait()
        .map_err(|e| CliError::local(format!("`{shown}` did not finish: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::local(format!(
            "`{shown}` failed ({status}); the commands before it ran"
        )))
    }
}

// ---------------------------------------------------------------------------
// oml doctor
// ---------------------------------------------------------------------------

/// The user whose root holds `node`, a workspace read: the node's own name
/// when it is a user root, else the first label under `universe` when the
/// qualified name `<user>.<name>` reads back as the same node, which it
/// does only inside a user root and which a tenant key resolves (ADR-0044
/// d5). `None` for a shared workspace.
fn user_root_holding(client: &Client, node: &Value) -> Option<String> {
    let name = node["name"].as_str()?;
    if node["user_root"] == true {
        return Some(name.to_owned());
    }
    let mut labels = node["path"].as_str()?.split('.');
    let (Some(_), Some(user), Some(_)) = (labels.next(), labels.next(), labels.next()) else {
        return None;
    };
    let (_, qualified) = client.get(&format!("/v2/workspaces/{user}.{name}")).ok()?;
    (qualified["id"] == node["id"]).then(|| user.to_owned())
}

/// A memory is started in a user's root by that user's keys only (ADR-0044
/// d16): a tenant key that writes, a contributor or a submitter, whose
/// default workspace lies in `user`'s root
/// is refused every new memory it does not address elsewhere, so the line
/// warns and names the fix. Nothing for a user key, whose default lies in
/// its own root, or for a key that writes nothing.
fn user_root_writes_doctor(
    me: Option<&Value>,
    default_node: Option<&Value>,
    user: Option<&str>,
) -> Option<String> {
    let (me, node, user) = (me?, default_node?, user?);
    let writes = me["role"] == "contributor" || me["role"] == "submitter";
    (me["user"].is_null() && writes).then(|| {
        format!(
            "warn: the default workspace {} lies in the root of user {user}, where only \
             {user}'s keys start memories; this tenant key's new memories there are refused \
             (submission.user_root_refused); issue the agent a key with --user {user}",
            node["path"].as_str().unwrap_or("of this key"),
        )
    })
}

/// The user-root lines (ADR-0085 d17), from every node of the tenant's tree
/// an admin key reads: a user root and every workspace under it stay
/// `development`, so a node there at another status, which a build before
/// that cap admitted, is named; it takes no further move, and the operator
/// decides whether it stays. Nothing when every such node is
/// `development`.
fn user_root_ladder_doctor(nodes: &[Value], incomplete: Option<&str>) -> Vec<String> {
    let roots: Vec<(&str, &str)> = nodes
        .iter()
        .filter(|n| n["user_root"] == true)
        .filter_map(|n| Some((n["path"].as_str()?, n["name"].as_str()?)))
        .collect();
    let mut lines: Vec<String> = nodes
        .iter()
        .filter(|n| n["status"].as_str().is_some_and(|s| s != "development"))
        .filter_map(|n| {
            let path = n["path"].as_str()?;
            let (_, user) = roots.iter().find(|(root, _)| {
                path == *root
                    || path
                        .strip_prefix(root)
                        .is_some_and(|rest| rest.starts_with('.'))
            })?;
            Some(format!(
                "warn user_root_status: {path} is {}, inside the root of user {user}, where \
                 every workspace stays development; it takes no further move \
                 (workspace.transition_refused, user_root), and the operator decides whether \
                 it stays",
                n["status"].as_str().unwrap_or("not development"),
            ))
        })
        .collect();
    if let Some(reason) = incomplete {
        lines.push(format!(
            "warn user_root_status: tree read incomplete after {} node(s) ({reason}), so a \
             user-root node off development may be unnamed",
            nodes.len()
        ));
    }
    lines
}

/// Every node of the tree at `tree`, one page after another, and why the
/// read stopped early when a page was refused or failed: the nodes read
/// before it, and the reason, which doctor reports and does not fail on.
fn tree_pages(client: &Client, tree: &str) -> (Vec<Value>, Option<String>) {
    let mut nodes = Vec::new();
    let mut path = tree.to_owned();
    loop {
        match client.get(&path) {
            Ok((_, page)) => {
                nodes.extend(page["items"].as_array().into_iter().flatten().cloned());
                match page["next_cursor"].as_str() {
                    Some(cursor) => path = format!("{tree}?cursor={}", encode(cursor)),
                    None => return (nodes, None),
                }
            }
            Err(e) => return (nodes, Some(unread_reason(e))),
        }
    }
}

/// The body `oml keys limit` sends: each limit it was told to set or to
/// clear, and nothing for one it was not told about, which the server then
/// keeps (ADR-0079 d5). No flag at all is refused here, before anything is
/// sent.
fn key_limits_body(
    expires: Option<&str>,
    no_expiry: bool,
    requests_per_minute: Option<u32>,
    no_rate_limit: bool,
) -> Result<Value, String> {
    let mut body = serde_json::Map::new();
    match (expires, no_expiry) {
        (Some(instant), _) => {
            body.insert("expiry".to_owned(), json!(instant));
        }
        (None, true) => {
            body.insert("expiry".to_owned(), Value::Null);
        }
        (None, false) => {}
    }
    match (requests_per_minute, no_rate_limit) {
        (Some(rate), _) => {
            body.insert("requests_per_minute".to_owned(), json!(rate));
        }
        (None, true) => {
            body.insert("requests_per_minute".to_owned(), Value::Null);
        }
        (None, false) => {}
    }
    if body.is_empty() {
        return Err(
            "refused: name a limit to set or clear with --expires, --no-expiry, \
             --requests-per-minute or --no-rate-limit; nothing was sent"
                .to_owned(),
        );
    }
    Ok(Value::Object(body))
}

/// The key lines (ADR-0079 d7), for the key doctor runs under: that the
/// server refused it as expired or past its rate; else, from `whoami`, the
/// instant it ends and the rate it is held to, and
/// `key_contributor_without_recall` for a contributor without recall as
/// `whoami` serves it: a submitter grant on a node no reader grant of the key
/// reaches, such as a v0.8.3 writer key issued write-only, which
/// works as before and is replaced by a submitter key (ADR-0093 d3). A key with
/// neither limit, and a refusal for any other reason, print nothing here.
fn key_doctor(me: Option<&Result<Value, CliError>>) -> Vec<String> {
    let mut lines = Vec::new();
    match me {
        Some(Err(CliError::Refused { envelope, .. })) => {
            let error = &envelope["error"];
            if error["code"] == "auth.invalid" && error["details"]["reason"] == "expired" {
                lines.push(format!(
                    "fail expired: this key ended at {}; an admin moves its date with `oml keys \
                     limit <key> --expires <instant>`, or issues another key",
                    error["details"]["expiry"].as_str().unwrap_or("its expiry")
                ));
            }
            if error["code"] == "limit.rate" {
                lines.push(format!(
                    "warn rate: this key reached its {} requests per minute; the next minute \
                     admits it again",
                    error["details"]["limit"]
                ));
            }
        }
        Some(Ok(me)) => {
            if let Some(expiry) = me["expiry"].as_str() {
                lines.push(format!(
                    "ok expiry: this key ends at {expiry}; an admin moves its date with `oml \
                     keys limit <key> --expires <instant>`"
                ));
            }
            if let Some(rate) = me["requests_per_minute"].as_u64() {
                lines.push(format!(
                    "ok rate: this key is held to {rate} requests per minute, counted by each \
                     server process alone"
                ));
            }
            // A rotated key's previous value still works until its grace
            // ends (ADR-0096 d13): information, not a fault.
            if let Some(until) = me["rotation_grace_until"].as_str() {
                lines.push(format!(
                    "info key.rotation_grace_open: this key's previous value still \
                     authenticates until {until}; move every client to the new value before \
                     then"
                ));
            }
            // The server decides it: `whoami`'s grants carry no ancestry.
            if me["without_recall"] == true {
                lines.push(format!(
                    "warn key_contributor_without_recall: {} is a contributor key that writes \
                     where no reader grant of it reaches, a writer key issued with --write-only \
                     before v0.8.4; it works as before, but issue a submitter key with `oml keys \
                     issue-agent <name> --role submitter` and cut this one off",
                    me["name"].as_str().unwrap_or("this key")
                ));
            }
        }
        _ => {}
    }
    lines
}

/// Whether this key may review in bulk and approve instructions, for a
/// reviewer key only (ADR-0055 d1, d13; ADR-0080 d16): a key a person holds
/// may; one used by an agent may not review in bulk and is refused an
/// instruction's approval, and one used by a service reviews nothing at all
/// (access.verb_ceiling, ADR-0025 d1). A person holding such a key otherwise
/// learns it mid-import, so the line warns and names the fix. A person's key
/// with a crew or a profile reviews in bulk and is refused an instruction's
/// approval.
fn review_doctor(me: Option<&Value>) -> Option<String> {
    let me = me?;
    if me["role"] != "reviewer" {
        return None;
    }
    let person = me["used_by"] == "person";
    let instructions = person && me["crew"].is_null() && me["profile"].is_null();
    Some(match (person, instructions) {
        (true, true) => "ok bulk review: this key lists and judges held items in bulk (oml review list, oml review approve --ids), steering and guardrails included".to_owned(),
        (true, false) => "warn instructions: this key has a crew or a profile, so its approval of a steering or guardrail version is refused (review.person_required, instruction_needs_person); use a person's reviewer key with neither".to_owned(),
        (false, _) if me["used_by"] == "service" => "warn review: this key is used by \
             service, so it reviews nothing (access.verb_ceiling); use a reviewer key issued \
             --used-by person or the admin page"
            .to_owned(),
        (false, _) => format!(
            "warn bulk review: this key is used by {}, so it cannot review in bulk \
             (access.verb_ceiling, bulk_review_is_person_only) or approve a steering or \
             guardrail version (review.person_required, instruction_needs_person); use a \
             reviewer key issued --used-by person or the admin page",
            me["used_by"].as_str().unwrap_or("unknown")
        ),
    })
}

/// The in-progress lines (ADR-0030 d14): whether the default workspace lies
/// inside an in-progress workspace, so recall serves unreviewed memories
/// there; for a person's reviewer key, that it may set the mode and how many
/// held items it can see blocking the commit; for a curator key, that it
/// acts only inside one.
fn in_progress_doctor(
    me: Option<&Value>,
    default_node: Option<&Value>,
    held_blocking: Option<usize>,
) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(me) = me else {
        return lines;
    };
    let root = default_node.and_then(|n| n["in_progress_root"]["name"].as_str());
    match root {
        Some(root) => lines.push(format!(
            "ok in progress: the default workspace is inside {root}, whose drafts are visible to \
             everyone working here; recall includes labelled unreviewed memories, which stay \
             inside until a person makes the drafts visible to the writing key again"
        )),
        None if me["profile"] == "curator" => lines.push(
            "warn curator: the default workspace is not in progress; curator keys act only \
             inside in-progress workspaces (access.verb_ceiling, curator_outside_in_progress)"
                .to_owned(),
        ),
        None => {}
    }
    if me["role"] == "reviewer" && me["used_by"] == "person" && me["profile"].is_null() {
        lines.push(
            "ok mode: set workspace mode with `oml workspaces mode <workspace> \
             in_progress|standard`"
                .to_owned(),
        );
        if let (Some(root), Some(n)) = (root, held_blocking)
            && n > 0
        {
            lines.push(format!(
                "warn commit: {n} held item(s) block committing {root}; review them with \
                 `oml review list --workspace {root}` (workspace.held_remaining)"
            ));
        }
    }
    lines
}

/// `oml doctor` checks the pair alone (ADR-0039 d15): which source set the
/// URL and which set the key, never the key; `/healthz` and `/readyz` at
/// the first source's URL, keyless, so a URL alone is enough to probe; and
/// `GET /v2/whoami` and the events tail only when that same source also
/// holds the key. No URL in the first source sends nothing and fails; a URL
/// without its key warns, naming the missing setting.
fn run_doctor(cli: &Cli, stored: &Credentials) -> Result<ExitCode, CliError> {
    let source = first_source(cli, stored);
    let url_source = source
        .as_ref()
        .filter(|s| s.url.is_some())
        .map(|s| s.origin(s.url_name));
    let key_source = source
        .as_ref()
        .filter(|s| s.key.is_some())
        .map(|s| s.origin(s.key_name));
    let next = match &source {
        None => Some(missing_message(NO_PAIR)),
        Some(s) if s.url.is_none() || s.key.is_none() => Some(missing_message(&s.half())),
        Some(_) => None,
    };

    let mut health = None;
    let mut ready = None;
    let mut me: Option<Result<Value, CliError>> = None;
    let mut head_seq: Option<u64> = None;
    // A user key reads universe's own claims through its node grant; one
    // that cannot is warned about (ADR-0044 d14). A submitter key holds no
    // such grant and is not asked (ADR-0093 d2).
    let mut reads_universe: Option<bool> = None;
    let mut admin_emails: Option<(usize, Vec<String>)> = None;
    let mut signup_overview: Option<Value> = None;
    // The rescan window an admin key reads: `None` for any other key, and
    // the reason where an admin's read failed.
    let mut rescan_read: Option<Result<Value, String>> = None;
    let mut queue_read: Option<Result<Value, String>> = None;
    let mut default_node: Option<Value> = None;
    // The user whose root holds a tenant key's default workspace, for a key
    // that writes
    // (ADR-0044 d16).
    let mut default_in_root_of: Option<String> = None;
    let mut waiting_edges: Option<usize> = None;
    let mut search_settings: Option<Value> = None;
    let mut federation_settings: Option<Value> = None;
    let mut usage_drift: Option<Result<Value, String>> = None;
    // The chat workspace's path and whether the server takes origin_kinds
    // (ADR-0063 d13).
    let mut chat_server: Vec<String> = Vec::new();
    // The skill lines from the key's offered set (ADR-0080 Consequences).
    let mut skills: Vec<String> = Vec::new();
    // The document lines (ADR-0081 Consequences).
    let mut documents: Vec<String> = Vec::new();
    // Whether the server serves the builder's preview (ADR-0082
    // Consequences).
    let mut builder: Vec<String> = Vec::new();
    let mut held_blocking: Option<usize> = None;
    // Every node of the tenant's tree, read by an admin key for the
    // user-root lines (ADR-0085 d17).
    let mut tree_nodes: Vec<Value> = Vec::new();
    let mut tree_incomplete: Option<String> = None;
    if let Some(url) = source.as_ref().and_then(|s| s.url) {
        let keyless = Client::new(url.clone(), None);
        health = Some(keyless.probe("/healthz")?);
        ready = Some(keyless.probe("/readyz")?);
        if let Some(key) = source.as_ref().and_then(|s| s.key) {
            let client = Client::new(url.clone(), Some(key.clone()));
            me = Some(client.whoami());
            chat_server = chat_doctor(&client, me.as_ref().and_then(|m| m.as_ref().ok()));
            skills = oml_cli::skills::server_lines(
                &client
                    .get("/v1/skills?include_own_held=true")
                    .map(|(_, page)| page),
            );
            documents = document_doctor(&client);
            // An empty body: refused inside the answer by a server that
            // serves preview, and never written by any server.
            builder = oml_cli::builder::server_lines(
                &client
                    .post(
                        "/v1/memories/preview",
                        &serde_json::json!({}),
                        &CallOpts {
                            auth: true,
                            idempotency: false,
                        },
                    )
                    .map(|(_, answer)| answer),
            );
            // A submitter key recalls nothing and is issued no universe
            // grant, so it is not asked (ADR-0093 d2).
            if let Some(Ok(v)) = &me
                && v["user"].is_object()
                && v["role"] != "submitter"
            {
                reads_universe = Some(match client.get("/v2/workspaces/universe") {
                    Ok(_) => true,
                    Err(CliError::Refused { .. }) => false,
                    Err(other) => return Err(other),
                });
            }
            // The default workspace's brief and waiting transitions, which a
            // session start and a ratifier depend on (ADR-0046 d9).
            if let Some(Ok(v)) = &me
                && let Some(id) = v["default_workspace"]["id"].as_str()
                && let Ok(segment) = segment(id)
            {
                default_node = client
                    .get(&format!("/v2/workspaces/{segment}"))
                    .ok()
                    .map(|(_, n)| n);
                if v["user"].is_null()
                    && (v["role"] == "contributor" || v["role"] == "submitter")
                    && let Some(node) = &default_node
                {
                    default_in_root_of = user_root_holding(&client, node);
                }
                // What a person's reviewer key sees held under the
                // in-progress root, one page (ADR-0030 d14).
                if v["role"] == "reviewer"
                    && v["used_by"] == "person"
                    && let Some(root) = default_node
                        .as_ref()
                        .and_then(|n| n["in_progress_root"]["id"].as_str())
                {
                    held_blocking = client
                        .get(&format!("/v2/review/held?workspace={root}&limit=200"))
                        .ok()
                        .and_then(|(_, page)| page["items"].as_array().map(Vec::len));
                }
                // The claim edges there waiting for a second key, one page
                // (ADR-0057 d17; ADR-0046 d4).
                waiting_edges = client
                    .get(&format!(
                        "/v2/edges?kind=claim&workspace_id={segment}&limit=200"
                    ))
                    .ok()
                    .map(|(_, page)| {
                        page["items"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|e| e["state"] == "pending")
                            .count()
                    });
            }
            head_seq = client
                .get("/v2/events?after_seq=0&limit=1")
                .ok()
                .and_then(|(_, v)| v["head_seq"].as_u64());
            // The allowlist's admin entries, read by an admin key where the
            // pages are on (ADR-0045 d18).
            let pages_on = ready
                .as_ref()
                .is_some_and(|(_, body)| body["web"].is_object());
            let is_admin = matches!(&me, Some(Ok(v)) if v["role"] == "admin");
            if pages_on && is_admin {
                admin_emails = count_admin_emails(&client);
                // Open sign-up's people and requests (ADR-0068 d11).
                signup_overview = client.get("/v2/web/users").ok().map(|(_, v)| v);
            }
            // The tenant's search default, read by an admin key (ADR-0049
            // d8).
            if is_admin {
                (tree_nodes, tree_incomplete) = tree_pages(&client, "/v2/workspaces/universe/tree");
                search_settings = client.get("/v2/search").ok().map(|(_, v)| v);
                // The tenant's federation policy and its wide keys
                // (ADR-0072 d12, Consequences).
                federation_settings = client.get("/v2/federation").ok().map(|(_, v)| v);
                // One event's window, read for the counts it carries
                // (ADR-0074 d28).
                rescan_read = Some(
                    client
                        .get("/v2/admin/rescan?limit=1")
                        .map(|(_, v)| v)
                        .map_err(unread_reason),
                );
                // The held instructions no person can approve, per
                // workspace (ADR-0080 d16; #435).
                queue_read = Some(
                    client
                        .get("/v2/review/queue")
                        .map(|(_, v)| v)
                        .map_err(unread_reason),
                );
                // The usage counters against their rows (ADR-0071 d3, d5).
                usage_drift = Some(
                    client
                        .get("/v2/admin/usage/drift")
                        .map(|(_, v)| v)
                        .map_err(unread_reason),
                );
            }
        }
    }
    // Every count of the lead's traffic is the metrics listener's alone
    // (ADR-0042 d11), read where OML_METRICS_URL names it, never /readyz,
    // which answers anyone (ADR-0052 d12; ADR-0054 d1).
    let scrape = std::env::var("OML_METRICS_URL")
        .ok()
        .filter(|u| !u.trim().is_empty())
        .and_then(|u| Scrape::fetch(u.trim_end_matches('/')));
    let mut web = web_doctor(ready.as_ref(), admin_emails, scrape.as_ref());
    web.extend(signup_doctor(signup_overview.as_ref()));
    let web_warn = web.iter().any(|line| line.starts_with("warn"));
    let mut search = search_doctor(ready.as_ref(), search_settings.as_ref(), scrape.as_ref());
    search.extend(federation_doctor(federation_settings.as_ref()));
    let search_warn = search.iter().any(|line| line.starts_with("warn"));
    let review = review_doctor(me.as_ref().and_then(|m| m.as_ref().ok()));
    let user_root_writes = user_root_writes_doctor(
        me.as_ref().and_then(|m| m.as_ref().ok()),
        default_node.as_ref(),
        default_in_root_of.as_deref(),
    );
    let in_progress = in_progress_doctor(
        me.as_ref().and_then(|m| m.as_ref().ok()),
        default_node.as_ref(),
        held_blocking,
    );
    let in_progress_warn = in_progress.iter().any(|line| line.starts_with("warn"));
    let user_root_status = user_root_ladder_doctor(&tree_nodes, tree_incomplete.as_deref());
    let telemetry = telemetry_doctor(scrape.as_ref());
    let journal = former_names_doctor(ready.as_ref());
    let mut scan = scan_doctor(ready.as_ref(), scrape.as_ref());
    scan.extend(scan_backlog_doctor(rescan_read.as_ref()));
    let scan_warn = scan.iter().any(|line| line.starts_with("warn"));
    let mut approvals = approval_doctor(rescan_read.as_ref());
    approvals.extend(instructions_unreviewable_doctor(queue_read.as_ref()));
    let approvals_warn = approvals.iter().any(|line| line.starts_with("warn"));
    let withdrawals = withdrawn_queue_doctor(rescan_read.as_ref());
    let withdrawals_fail = withdrawals.iter().any(|line| line.starts_with("fail"));
    let withdrawals_warn = withdrawals.iter().any(|line| line.starts_with("warn"));
    let mut lifecycle = lifecycle_doctor(rescan_read.as_ref());
    lifecycle.extend(public_doctor(rescan_read.as_ref()));
    lifecycle.extend(flagged_creates_doctor(rescan_read.as_ref()));
    let lifecycle_warn = lifecycle.iter().any(|line| line.starts_with("warn"));
    let lifecycle_fail = lifecycle.iter().any(|line| line.starts_with("fail"));
    let telemetry_warn = telemetry.iter().any(|line| line.starts_with("warn"));
    let usage = usage_doctor(usage_drift.as_ref());
    let usage_warn = usage.iter().any(|line| line.starts_with("warn"));
    // The calling key's expiry and rate; an expired key already fails the
    // whoami line (ADR-0079 d7).
    let key_limits = key_doctor(me.as_ref());
    let key_limits_warn = key_limits.iter().any(|line| line.starts_with("warn"));
    let handlers = handlers_doctor(scrape.as_ref());
    let handlers_warn = handlers.iter().any(|line| line.starts_with("warn"));
    let uploads = uploads_doctor(scrape.as_ref());
    let uploads_warn = uploads.iter().any(|line| line.starts_with("warn"));
    let imports = imports_doctor(scrape.as_ref());
    let imports_warn = imports.iter().any(|line| line.starts_with("warn"));
    let exports = exports_doctor(scrape.as_ref());
    // The sync failures of ADR-0057 d17 that the clone alone answers; the
    // stale count and the waiting edges are the sources lines'.
    let sync = oml_cli::sync::doctor::lines(
        source.as_ref().and_then(|s| s.url.map(String::as_str)),
        me.as_ref()
            .and_then(|m| m.as_ref().ok())
            .and_then(|m| m["shared"].as_bool()),
        &std::env::current_dir().unwrap_or_default(),
    );
    let sync_warn = sync.iter().any(|line| line.starts_with("warn"));
    // The chat hooks' local state (ADR-0063 d13).
    let mut chat = oml_cli::chat::doctor_lines(&|name: &str| std::env::var(name).ok());
    chat.extend(chat_server);
    let chat_warn = chat.iter().any(|line| line.starts_with("warn"));
    skills.extend(local_skill_lines());
    let skills_warn = skills.iter().any(|line| line.starts_with("warn"));
    let documents_warn = documents.iter().any(|line| line.starts_with("warn"));
    let builder_warn = builder.iter().any(|line| line.starts_with("warn"));
    let sources = sources_doctor(
        server_version(ready.as_ref()).as_deref(),
        default_node.as_ref(),
        waiting_edges,
    );
    let sources_warn = sources.iter().any(|line| line.starts_with("warn"));
    // The connection surface, from wherever the CLI runs (ADR-0031 d14).
    let oauth = source
        .as_ref()
        .and_then(|s| s.url)
        .zip(ready.as_ref())
        .map(|(url, (_, body))| discovery::check(url, body));
    let oauth_ok = !oauth.as_ref().is_some_and(|o| o["state"] == "failing");
    // A lead with no oauth.issuer cannot show agents the URL to set beside
    // their keys; an admin can set it (ADR-0076 d6, d8).
    let server_origin = (oauth.as_ref().is_some_and(|o| o["state"] == "off")
        && matches!(&me, Some(Ok(v)) if v["role"] == "admin"))
    .then_some(SERVER_ORIGIN);
    // The plugin's key option exported with no url reaches no server, since
    // its url has no default (ADR-0076 d8).
    let plugin = plugin_no_server(&|name: &str| std::env::var(name).ok());
    let probes_ok = matches!((&health, &ready), (Some((200, _)), Some((200, _))));
    let whoami_ok = !matches!(me, Some(Err(_)));
    let review_warn = review.as_ref().is_some_and(|line| line.starts_with("warn"));
    let outcome = if !probes_ok || !whoami_ok || !oauth_ok || withdrawals_fail || lifecycle_fail {
        "fail"
    } else if me.is_none()
        || server_origin.is_some()
        || plugin.is_some()
        || reads_universe == Some(false)
        || user_root_writes.is_some()
        || web_warn
        || search_warn
        || review_warn
        || telemetry_warn
        || handlers_warn
        || uploads_warn
        || imports_warn
        || scan_warn
        || approvals_warn
        || withdrawals_warn
        || lifecycle_warn
        || sync_warn
        || chat_warn
        || skills_warn
        || documents_warn
        || builder_warn
        || sources_warn
        || in_progress_warn
        || !user_root_status.is_empty()
        || usage_warn
        || key_limits_warn
    {
        "warn"
    } else {
        "ok"
    };
    let whoami = match &me {
        Some(Ok(v)) => json!({
            "role": v["role"],
            "tenant": v["tenant"]["name"],
            "user": v["user"]["name"],
            "default_workspace": v["default_workspace"]["path"],
        }),
        Some(Err(e)) => json!({ "error": refusal(e) }),
        None => Value::Null,
    };
    let probe_json = |p: &Option<(u16, Value)>| {
        p.as_ref().map_or(
            Value::Null,
            |(status, body)| json!({ "status": status, "body": body }),
        )
    };

    if cli.json {
        print_json(&json!({
            "url_source": url_source,
            "key_source": key_source,
            "healthz": probe_json(&health),
            "readyz": probe_json(&ready),
            "whoami": whoami,
            "chain_head_seq": head_seq,
            "reads_universe": reads_universe,
            "user_root_writes": user_root_writes,
            "default_workspace": default_node.as_ref().map(|n| json!({
                "path": n["path"],
                "brief_items": n["brief"]["items"].as_array().map_or(0, Vec::len),
                "brief_truncated": n["brief"]["budget"]["truncated"],
                "pending_transitions": n["pending_transitions"].as_array().map_or(0, Vec::len),
            })),
            "oauth_discovery": oauth,
            "server_origin": server_origin,
            "plugin": plugin,
            "web": web,
            "search": search,
            "review": review,
            "sync": sync,
            "chat": chat,
            "skills": skills,
            "documents": documents,
            "builder": builder,
            "telemetry": telemetry,
            "journal": journal,
            "handlers": handlers,
            "uploads": uploads,
            "imports": imports,
            "exports": exports,
            "scan": scan,
            "usage": usage,
            "key_limits": key_limits,
            "approvals": approvals,
            "withdrawals": withdrawals,
            "lifecycle": lifecycle,
            "sources": sources,
            "in_progress": in_progress,
            "user_root_status": user_root_status,
            "client_version": env!("CARGO_PKG_VERSION"),
            "server_version": server_version(ready.as_ref()),
            "verdict": outcome,
            "next": next,
        }));
    } else {
        match &url_source {
            Some(source) => println!("url         : {source}"),
            None => println!("url         : {NO_SERVER_DOCTOR}"),
        }
        println!("api key     : {}", key_source.as_deref().unwrap_or("unset"));
        if let Some(line) = plugin {
            println!("plugin      : {line}");
        }
        if let Some((status, body)) = &health {
            println!(
                "healthz     : {} ({})",
                verdict(*status == 200),
                field(body, "status")
            );
        }
        if let Some((status, body)) = &ready {
            println!("readyz      : {}", verdict(*status == 200));
            if let Some(checks) = body["checks"].as_object() {
                for (name, state) in checks {
                    println!("  {name:<10}: {}", state.as_str().unwrap_or("—"));
                }
            }
        }
        if let Some(line) = &journal {
            println!("journal     : {line}");
        }
        match &me {
            Some(Ok(_)) => println!(
                "whoami      : role {}, tenant {}, default workspace {}",
                whoami["role"].as_str().unwrap_or("—"),
                whoami["tenant"].as_str().unwrap_or("—"),
                whoami["default_workspace"].as_str().unwrap_or("—"),
            ),
            Some(Err(e)) => println!("whoami      : fail ({})", refusal(e)),
            None => println!("whoami      : not called (no key with this URL)"),
        }
        for line in &key_limits {
            println!("key         : {line}");
        }
        if let Some(Ok(v)) = &me {
            match v["user"]["name"].as_str() {
                Some(user) => println!("user        : {user} (root universe.{user})"),
                None => println!(
                    "user        : none (tenant key: names resolve in the shared namespace)"
                ),
            }
        }
        if reads_universe == Some(false) {
            println!("universe    : warn: this key does not read universe's claims");
        }
        if let Some(line) = &user_root_writes {
            println!("user root   : {line}");
        }
        for line in &user_root_status {
            println!("user root   : {line}");
        }
        if let Some(n) = &default_node {
            let items = n["brief"]["items"].as_array().map_or(0, Vec::len);
            if items == 0 {
                println!(
                    "brief       : empty: no ratified brief edge on {}; a reviewer proposes one with oml edges propose brief",
                    field(n, "path")
                );
            } else {
                println!(
                    "brief       : {items} item(s) on {}{}",
                    field(n, "path"),
                    if n["brief"]["budget"]["truncated"] == true {
                        ", more cut by brief.max_tokens"
                    } else {
                        ""
                    }
                );
            }
            let waiting = n["pending_transitions"].as_array().map_or(0, Vec::len);
            if waiting > 0 {
                println!(
                    "transitions : {waiting} waiting for a second identity; oml workspaces status pending {}",
                    field(n, "path")
                );
            }
        }
        match head_seq {
            Some(seq) => println!("chain head  : seq {seq}"),
            None => println!("chain head  : unavailable (needs a key with events-tail access)"),
        }
        match &oauth {
            Some(o) if o["state"] == "off" => {
                println!("oauth       : off (no oauth.issuer: /mcp takes keys only)");
            }
            Some(o) if o["state"] == "ok" => println!(
                "oauth       : ok (issuer {} serves both discovery documents)",
                field(o, "issuer")
            ),
            Some(o) => println!(
                "oauth       : failing ({}: {})",
                field(o, "url"),
                field(o, "reason")
            ),
            None => {}
        }
        if let Some(line) = server_origin {
            println!("oauth       : {line}");
        }
        for line in &web {
            println!("web         : {line}");
        }
        for line in &search {
            println!("search      : {line}");
        }
        if let Some(line) = formats_doctor(ready.as_ref()) {
            println!("formats     : {line}");
        }
        if let Some(line) = &review {
            println!("review      : {line}");
        }
        for line in &telemetry {
            println!("telemetry   : {line}");
        }
        for line in &handlers {
            println!("handlers    : {line}");
        }
        for line in &uploads {
            println!("uploads     : {line}");
        }
        for line in &imports {
            println!("imports     : {line}");
        }
        for line in &exports {
            println!("exports     : {line}");
        }
        for line in &scan {
            println!("scan        : {line}");
        }
        for line in &usage {
            println!("usage       : {line}");
        }
        for line in &approvals {
            println!("approvals   : {line}");
        }
        for line in &withdrawals {
            println!("withdrawals : {line}");
        }
        for line in &lifecycle {
            println!("lifecycle   : {line}");
        }
        for line in &sync {
            println!("sync        : {line}");
        }
        for line in &chat {
            println!("chat        : {line}");
        }
        for line in &skills {
            println!("skills      : {line}");
        }
        for line in &documents {
            println!("documents   : {line}");
        }
        for line in &builder {
            println!("builder     : {line}");
        }
        for line in &sources {
            println!("sources     : {line}");
        }
        for line in &in_progress {
            println!("in progress : {line}");
        }
        println!("client      : oml {}", env!("CARGO_PKG_VERSION"));
        match (&ready, server_version(ready.as_ref())) {
            (_, Some(version)) => println!("server      : oml-server-lead {version}"),
            (Some(_), None) => println!("server      : version not reported by /readyz"),
            (None, None) => {}
        }
        println!("verdict     : {outcome}");
        if let Some(next) = &next {
            println!("next        : {next}");
        }
    }
    Ok(if outcome == "fail" {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// How many of the tenant's allowlist entries open the admin page, read
/// through every page of `GET /v2/web/emails`; `None` when the listing is
/// refused or unreadable.
fn count_admin_emails(client: &Client) -> Option<(usize, Vec<String>)> {
    let mut admins = 0;
    let mut users: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut path = "/v2/web/emails?limit=200".to_owned();
    loop {
        let (_, page) = client.get(&path).ok()?;
        let items = page["items"].as_array()?;
        admins += items
            .iter()
            .filter(|e| e["admin"].as_bool() == Some(true))
            .count();
        users.extend(
            items
                .iter()
                .filter_map(|e| e["user"].as_str().map(ToOwned::to_owned)),
        );
        match page["next_cursor"].as_str() {
            Some(cursor) => path = format!("/v2/web/emails?limit=200&cursor={}", encode(cursor)),
            None => break,
        }
    }
    // A user whose `<user>-web` session key is absent: its first sign-in
    // has not happened or failed part way (ADR-0045 d18).
    let keyless = users
        .into_iter()
        .filter(|user| {
            matches!(
                client.get(&format!("/v2/keys/{user}-web")),
                Err(CliError::Refused { status: 404, .. })
            )
        })
        .collect();
    Some((admins, keyless))
}

/// The search doctor lines (ADR-0049 d8): the deployment's default and
/// the served pipelines from `/readyz`; a warning when the default is the
/// comparison pipeline `keyword-all`; and, read by an admin key, a warning
/// when the tenant's journalled default is one the deployment no longer
/// serves, so the deployment's applies instead. A pipeline whose retriever
/// the database lacks (`pg_trgm`) never reaches here: the lead refuses to
/// start, naming it.
/// The formats doctor line (ADR-0056 d14): the media types the lead serves
/// and the adapters, with their versions, that derive their search text.
fn formats_doctor(ready: Option<&(u16, Value)>) -> Option<String> {
    let (_, body) = ready?;
    let Some(enabled) = body["formats"]["enabled"].as_array() else {
        return Some("not reported by /readyz (a server before claim formats)".to_owned());
    };
    let adapters: Vec<&str> = body["formats"]["adapters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    Some(format!(
        "{} media types enabled; adapters {}",
        enabled.len(),
        adapters.join(", ")
    ))
}

/// The federation lines (ADR-0072 d12, Consequences), read by an admin
/// key: federation on with `max_sources` 0, each key whose recall met more
/// than 1,000 candidate sources since the lead started, and the widen
/// hint's signal failing or never built (#244).
fn federation_doctor(settings: Option<&Value>) -> Vec<String> {
    let Some(settings) = settings else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if settings["enabled"] == true && settings["max_sources"] == 0 {
        lines.push(
            "warn federation is on with max_sources 0: a wide recall searches its scope alone and every sources list is refused; set it with: oml search federation --max-sources 8"
                .to_owned(),
        );
    }
    for key in settings["wide_keys"].as_array().into_iter().flatten() {
        lines.push(format!(
            "warn key {} reads {} trusted workspaces outside its scopes, above 1,000: each recall of a federated tenant reads their counts, so its statistics read grows with them",
            key["key_id"].as_str().unwrap_or("?"),
            key["candidates"]
        ));
    }
    // The widen hint's signal (ADR-0072 d8, as built by #244): a build that
    // failed, or recalls that found none while none was ever built.
    let widen = &settings["widen_signal"];
    if widen["failed"].as_u64().unwrap_or(0) > 0 {
        lines.push(format!(
            "warn the widen hint's signal failed to build {} times since the lead started, last: {}; a scope recall without its signal carries no hint",
            widen["failed"],
            widen["last_failure"].as_str().unwrap_or("?")
        ));
    }
    if widen["evicted"].as_u64().unwrap_or(0) > 0 {
        lines.push(format!(
            "warn {} widen signals were dropped to keep this tenant or the lead within its share, each not rebuilt for 300 s: their scopes carry no hint meanwhile",
            widen["evicted"]
        ));
    }
    if settings["enabled"] == true
        && widen["built"].as_u64() == Some(0)
        && widen["unserved"].as_u64().unwrap_or(0) > 0
    {
        lines.push(format!(
            "warn {} scope recalls found no widen signal and none was built since the lead started: no scope answer carries the widen hint until one is",
            widen["unserved"]
        ));
    }
    lines
}

fn search_doctor(
    ready: Option<&(u16, Value)>,
    settings: Option<&Value>,
    scrape: Option<&Scrape>,
) -> Vec<String> {
    let Some((_, body)) = ready else {
        return Vec::new();
    };
    let Some(default) = body["search"]["default"].as_str() else {
        return vec!["not reported by /readyz (a server before 0.5.3)".to_owned()];
    };
    let enabled: Vec<&str> = body["search"]["enabled"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut lines = vec![format!("default {default}; enabled {}", enabled.join(", "))];
    // The reranker the reranking pipelines call, from /readyz, and the
    // stages it skipped since the lead started, by reason, from the metrics
    // listener (ADR-0053 d9; ADR-0054 d1).
    let rerank = &body["search"]["rerank"];
    if let Some(model) = rerank["model"].as_str() {
        let floor = rerank["floor"]
            .as_f64()
            .map_or_else(|| "off".to_owned(), |f| f.to_string());
        lines.push(format!(
            "rerank {model}, depth {}, floor {floor}",
            rerank["depth"].as_u64().unwrap_or(0)
        ));
        match scrape {
            None => lines.push(
                "rerank skips not read: set OML_METRICS_URL to the lead's metrics listener"
                    .to_owned(),
            ),
            Some(scrape) => {
                let count = |reason: &str| {
                    scrape.sum(
                        "oml_reranks_total",
                        &[("outcome", "skipped"), ("reason", reason)],
                    )
                };
                let (busy, timeout, failed) = (count("busy"), count("timeout"), count("failed"));
                if busy + timeout > 0 {
                    let reasons: Vec<String> = [("busy", busy), ("timeout", timeout)]
                        .into_iter()
                        .filter(|(_, n)| *n > 0)
                        .map(|(reason, n)| format!("{n} {reason}"))
                        .collect();
                    lines.push(format!(
                        "warn search_rerank: {} reranks skipped since start ({}), so those recalls served bm25's order; raise search.rerank.max_concurrent or timeout_ms, or lower search.rerank.depth",
                        busy + timeout,
                        reasons.join(", ")
                    ));
                }
                if failed > 0 {
                    lines.push(format!(
                        "warn search_rerank_failed: {failed} rerank failed since start, so those recalls served bm25's order; the lead's log names each reason under event rerank_failed"
                    ));
                }
            }
        }
    }
    lines.extend(sparse_doctor(body, &enabled, settings));
    if default == "keyword-all" {
        lines.push(
            "warn search: the default is keyword-all, every word exactly, kept for comparison; set search.default"
                .to_owned(),
        );
    }
    // A tenant default the deployment does not serve: recalls naming no
    // pipeline run the deployment's default, marked (ADR-0049 d15).
    if let Some(settings) = settings
        && let Some(tenant) = settings["tenant_default"].as_str()
        && settings["effective"].as_str() != Some(tenant)
    {
        lines.push(format!(
            "warn search_tenant_default: this tenant's default {tenant} is not served here, so a recall naming no pipeline runs {}, marked search.fallback tenant_default_unserved; oml search set-default <pipeline>, or serve {tenant}",
            settings["effective"].as_str().unwrap_or("the deployment's default")
        ));
    }
    let recall_counts = scrape.map(|scrape| {
        let pipeline = ("pipeline", "bm25-postings");
        (
            scrape.sum("oml_recalls_total", &[pipeline, ("access", "postings")]),
            scrape.sum("oml_recalls_total", &[pipeline, ("access", "scan")]),
        )
    });
    lines.extend(postings_doctor(recall_counts, settings));
    lines.extend(valid_time_doctor(&enabled, settings));
    lines
}

/// Events dropped above this share of the recalls answered since start
/// warn (ADR-0054 d13).
const DROPPED_WARN_PERCENT: u64 = 1;

/// The labeller's lag past which doctor warns, in seconds (ADR-0054 d13).
const LABELLER_LAG_WARN_SECONDS: u64 = 600;

/// The panic guard's doctor line, from the metrics listener alone: handler
/// panics since start, each a defect the lead's log names by request id.
fn handlers_doctor(scrape: Option<&Scrape>) -> Vec<String> {
    let Some(scrape) = scrape else {
        return vec!["not read: set OML_METRICS_URL to the lead's metrics listener".to_owned()];
    };
    let mut lines = match scrape.sum("oml_handler_panics_total", &[]) {
        0 => vec!["no handler panicked since start".to_owned()],
        n => vec![format!(
            "warn handler_panic: {n} request(s) hit a handler panic since start and were answered 500 system.internal (details.reason handler_panic); the lead's log names each under event handler_panic with its request id, and every one is a defect to report"
        )],
    };
    // A web console act claims its Idempotency-Key before it acts; one
    // whose claim was left unsettled answers its resubmissions
    // idempotency.in_progress instead of acting again (#233).
    if let n @ 1.. = scrape.sum("oml_idempotency_unsettled_total", &[]) {
        lines.push(format!(
            "warn idempotency_unsettled: {n} claimed act(s) (web console acts and key issues) since start left their Idempotency-Key claimed and unrecorded (the lead's log names each under event idempotency_unsettled with the act); a resubmission of that form answers 409 idempotency.in_progress and acts nothing, so nothing is repeated, and a fresh form acts under a fresh key. Look for Postgres errors at those times; operations/runbooks.md (a form answers idempotency.in_progress) lists the claims held and how old each is"
        ));
    }
    lines
}

/// The `exports` section (ADR-0084 d10, d15; reading I9): the tenant's
/// export policy, which is `own_root` while no route sets another, and the
/// bundles served since start, from the metrics listener.
fn exports_doctor(scrape: Option<&Scrape>) -> Vec<String> {
    let policy = "export_policy: own_root, the default: a person exports only inside their own \
                  root, in history mode; no route sets another policy in this build"
        .to_owned();
    match scrape {
        Some(scrape) if scrape.0.contains("oml_exports") => {
            let mut lines = vec![
                policy,
                format!(
                    "{} bundle(s) served since start, each journalled as workspace.exported",
                    scrape.sum("oml_exports_total", &[])
                ),
            ];
            // ADR-0084 I13: export.busy and export.too_large, and every other
            // refusal of the route, by code.
            let mut refused: Vec<(String, u64)> = scrape
                .samples("oml_export_refusals_total", &[])
                .filter_map(|(set, n)| {
                    set.split(',')
                        .find_map(|pair| pair.strip_prefix("code=\"")?.strip_suffix('"'))
                        .map(|code| (code.to_owned(), n))
                })
                .collect();
            refused.sort();
            if !refused.is_empty() {
                let list = refused
                    .iter()
                    .map(|(code, n)| format!("{code} {n}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!("export_refusals since start: {list}"));
            }
            lines
        }
        Some(_) => vec![
            policy,
            "off (this server serves no workspace export)".to_owned(),
        ],
        None => vec![
            policy,
            "bundles served: not read; set OML_METRICS_URL to the lead's metrics listener"
                .to_owned(),
        ],
    }
}

/// The `uploads` doctor lines (ADR-0004 d3, as amended 2026-11-17; #451),
/// from the metrics listener alone: the sessions by state and the bytes
/// stored at the last sweep; a warn for sessions whose key is shredded and
/// whose objects remain, for failed sweeps, and for the refusals by code
/// that say something is wrong (a digest mismatch, a failing store, a
/// limit reached). Nothing from a server before uploads.
fn uploads_doctor(scrape: Option<&Scrape>) -> Vec<String> {
    let Some(scrape) = scrape else {
        return vec!["not read: set OML_METRICS_URL to the lead's metrics listener".to_owned()];
    };
    if !scrape.0.contains("oml_upload_sessions") {
        return vec!["off (this server keeps no upload sessions)".to_owned()];
    }
    let state = |s: &str| scrape.sum("oml_upload_sessions", &[("state", s)]);
    let mut lines = vec![format!(
        "{} open, {} completed and unbound, {} bound, {} expired, {} abandoned, {} failed; {} bytes declared unbound, {} bytes stored",
        state("open"),
        state("completed"),
        state("bound"),
        state("expired"),
        state("abandoned"),
        state("failed"),
        scrape.sum("oml_upload_unbound_bytes", &[]),
        scrape.sum("oml_upload_stored_bytes", &[]),
    )];
    for (set, n) in scrape.samples("oml_upload_store_unavailable", &[]) {
        if let (1.., Some(kind)) = (
            n,
            set.strip_prefix("reason=\"")
                .and_then(|r| r.strip_suffix('"')),
        ) {
            lines.push(format!(
                "warn upload_store_unavailable: object store unavailable: {kind}; the lead serves every other route, every upload answers upload.storage_failed until storage.url names a directory the lead can write, and the next sweep tries it again; the lead's log names the path under event upload_store_unavailable (docs/operations/upgrades.md)"
            ));
        }
    }
    if let n @ 1.. = scrape.sum("oml_upload_unswept", &[]) {
        lines.push(format!(
            "warn upload_unswept: {n} upload session(s) have their key shredded and their objects still at storage.url; the ciphertext opens under no key, and each sweep tries the removal again: check storage.url's directory is writable by the lead (event upload_sweep_failed names the reason)"
        ));
    }
    if let n @ 1.. = scrape.sum("oml_upload_staging_stale", &[]) {
        lines.push(format!(
            "warn upload_staging_stale: {n} bound upload(s) still hold their staging key 24 hours after binding; an import's end shreds it and the next sweep shreds any whose import ended (event import_shred_failed names the reason) (ADR-0004 d3, ADR-0084 I11)"
        ));
    }
    if let n @ 1.. = scrape.sum("oml_upload_sweep_failures_total", &[]) {
        lines.push(format!(
            "warn upload_sweep_failed: {n} upload sweep step(s) failed since start; the lead's log names each under event upload_sweep_failed, and the next sweep retries"
        ));
    }
    let mut refused = Vec::new();
    for (set, n) in scrape.samples("oml_upload_refusals_total", &[]) {
        if let Some(code) = set
            .split(',')
            .find_map(|pair| pair.strip_prefix("code=\"")?.strip_suffix('"'))
        {
            refused.push((code.to_owned(), n));
        }
    }
    refused.sort();
    if !refused.is_empty() {
        let list = refused
            .iter()
            .map(|(code, n)| format!("{code} {n}"))
            .collect::<Vec<_>>()
            .join(", ");
        let alarming = refused.iter().any(|(code, _)| {
            matches!(
                code.as_str(),
                "upload.digest_mismatch" | "upload.storage_failed" | "upload.tenant_limit"
            )
        });
        lines.push(if alarming {
            format!(
                "warn upload_refusals since start: {list}; a digest mismatch is bytes changed on the way, a storage failure is storage.url failing, and the tenant limit is uploads.tenant_max_bytes reached (docs/api/errors.md, upload)"
            )
        } else {
            format!("upload_refusals since start: {list}")
        });
    }
    lines
}

/// The `imports` doctor lines (ADR-0084 d15; #426), from the metrics
/// listener alone: the imports by state at the last sweep; a warn for
/// imports stopped while running, whose position `GET /v2/imports/{id}`
/// answers; the verification refusals by code; and the imports that used
/// `allow_older`. Nothing from a server before imports.
fn imports_doctor(scrape: Option<&Scrape>) -> Vec<String> {
    let Some(scrape) = scrape else {
        return vec!["not read: set OML_METRICS_URL to the lead's metrics listener".to_owned()];
    };
    if !scrape.0.contains("oml_imports") {
        return vec!["off (this server runs no imports)".to_owned()];
    }
    let state = |s: &str| scrape.sum("oml_imports", &[("state", s)]);
    let stopped = scrape.sum("oml_imports_stopped", &[]);
    let mut lines = vec![format!(
        "{} done, {} failed, {} running, {stopped} stopped",
        state("done"),
        state("failed"),
        state("running"),
    )];
    // ADR-0084 I11: only a run past the hour that no process runs is
    // stopped; the next sweep ends it failed and shreds its upload.
    if stopped > 0 {
        lines.push(format!(
            "warn imports_stopped: {stopped} import(s) running for more than an hour that the lead is not running (it restarted mid-import); the next upload sweep ends each failed and shreds its upload's key, what each wrote stays held, GET /v2/imports/{{id}} names its position, and importing the same bundle again writes only what is missing"
        ));
    }
    if let n @ 1.. = scrape.sum("oml_imports_unprovenanced", &[]) {
        lines.push(format!(
            "warn imports_unprovenanced: {n} version(s) carry an import's idempotency key and no import_provenance; this build records both in one commit, so report it as a defect with the versions' ids (ADR-0084 I14)"
        ));
    }
    if let n @ 1.. = scrape.sum("oml_imports_allow_older", &[]) {
        lines.push(format!(
            "allow_older: {n} import(s) admitted a bundle older than the target's ledger (workspace.imported names each with allow_older: true)"
        ));
    }
    let mut refused = Vec::new();
    for (set, n) in scrape.samples("oml_import_refusals_total", &[]) {
        if let Some(code) = set
            .split(',')
            .find_map(|pair| pair.strip_prefix("code=\"")?.strip_suffix('"'))
        {
            refused.push((code.to_owned(), n));
        }
    }
    refused.sort();
    if !refused.is_empty() {
        let list = refused
            .iter()
            .map(|(code, n)| format!("{code} {n}"))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("import_refusals since start: {list}"));
    }
    lines
}

/// The recall telemetry's doctor lines, from the metrics listener alone
/// (ADR-0054 d13, as accepted): events dropped above 1% of recalls, by
/// reason; the writer's queue above half its bound; rows past the 30 days not
/// yet purged; the labeller's lag above 10 minutes. Each names its repair.
fn telemetry_doctor(scrape: Option<&Scrape>) -> Vec<String> {
    let Some(scrape) = scrape else {
        return vec!["not read: set OML_METRICS_URL to the lead's metrics listener".to_owned()];
    };
    if !scrape.0.contains("oml_telemetry_queue_depth") {
        return vec!["off (this server records no recall)".to_owned()];
    }
    let mut lines = Vec::new();
    let written = scrape.sum("oml_recall_events_total", &[("outcome", "written")]);
    let mut dropped = Vec::new();
    for reason in [
        "dropped_queue_full",
        "dropped_store_error",
        "dropped_no_partition",
    ] {
        let n = scrape.sum("oml_recall_events_total", &[("outcome", reason)]);
        if n > 0 {
            dropped.push((reason, n));
        }
    }
    let lost: u64 = dropped.iter().map(|(_, n)| n).sum();
    if lost * 100 > DROPPED_WARN_PERCENT * (written + lost) && lost > 0 {
        lines.push(format!(
            "warn telemetry_dropped: {lost} of {} recall events dropped since start ({}); the queue is full under load, or the store refuses; no_partition is a tenant the lead's next start gives its partitions",
            written + lost,
            dropped
                .iter()
                .map(|(r, n)| format!("{n} {}", r.trim_start_matches("dropped_")))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let depth = scrape.sum("oml_telemetry_queue_depth", &[]);
    if depth > 5_000 {
        lines.push(format!(
            "warn telemetry_backlog: {depth} events wait for the writer (bound 10000); past it they are dropped"
        ));
    }
    let overdue = scrape.sum("oml_telemetry_purge_overdue", &[]);
    if overdue > 0 {
        lines.push(format!(
            "warn telemetry_purge: {overdue} tenant(s) hold recall events past the 30 days and a minute, so the lead's purge is behind or failing; their rows are never read, and the lead's log names the store error"
        ));
    }
    let lag = scrape.sum("oml_labeller_lag_seconds", &[]);
    if lag > LABELLER_LAG_WARN_SECONDS {
        lines.push(format!(
            "warn telemetry_labeller: the labeller's last pass ended {lag} s ago (more than 600); labels and the purge wait for it"
        ));
    }
    if lines.is_empty() {
        lines.push(format!(
            "on: {written} recall events written since start, none dropped past 1%"
        ));
    }
    lines
}

/// The rescan backlog lines (ADR-0074 d28), from an admin key's one-event
/// rescan window: held versions an older rule set scanned, which stay their
/// authors' alone in in-progress workspaces until `--record-held` reads
/// them, and every stored version an older rule set scanned. A key that is
/// no admin's is told an admin key reads them; an admin key whose read
/// failed is told the reason, as the usage and approval lines are.
fn scan_backlog_doctor(read: Option<&Result<Value, String>>) -> Vec<String> {
    let r = match read {
        None => {
            return vec![
                "scan_held_unrescanned, scan_rescan_due: not read; an admin key reads them"
                    .to_owned(),
            ];
        }
        Some(Err(reason)) => {
            return vec![format!(
                "warn scan_held_unrescanned, scan_rescan_due: not read ({reason})"
            )];
        }
        Some(Ok(report)) => report,
    };
    let version = r["scanner_version"]
        .as_str()
        .unwrap_or("the server's rule set");
    let held = r["held_unrescanned"].as_u64().unwrap_or(0);
    let due = r["rescan_due"].as_u64().unwrap_or(0);
    let unlisted = r["findings_unlisted"].as_u64().unwrap_or(0);
    let mut lines = vec![
        if held > 0 {
            format!(
                "warn scan_held_unrescanned: {held} held version(s) were scanned under an older rule set than {version}, and stay their authors' alone in in-progress workspaces; run oml admin rescan --record-held"
            )
        } else {
            "scan_held_unrescanned: 0".to_owned()
        },
        if due > 0 {
            format!(
                "warn scan_rescan_due: {due} stored version(s) were scanned under an older rule set than {version}; oml admin rescan reports what it finds in them"
            )
        } else {
            "scan_rescan_due: 0".to_owned()
        },
    ];
    // A server before the findings list does not report the count.
    if r.get("findings_unlisted").is_some() {
        lines.push(if unlisted > 0 {
            format!(
                "warn scan_findings_unlisted: {unlisted} held version(s) carry a scan finding the findings list lacks, written by a server older than migration 0043; the review card shows only the stored summary for them; run oml admin rescan --record-held"
            )
        } else {
            "scan_findings_unlisted: 0".to_owned()
        });
    }
    lines
}

/// Why a read doctor made was not answered, as its lines name it: the
/// status and code of a refusal, else the local reason.
fn unread_reason(e: CliError) -> String {
    match e {
        CliError::Refused { status, code, .. } => format!("{status} {code}"),
        CliError::Local(reason) | CliError::Unanswered(reason) => reason,
    }
}

/// The approval line (ADR-0077 d6), from the same one-event rescan window:
/// how many served versions carry no approval because migration 0044's
/// fill could not match their verdict. A count, never a list, and no
/// command lists them. A server
/// before the count does not report it, and the line is then absent. An
/// admin key whose read failed is told so with the reason, and a count
/// that is no unsigned integer is named, never read as zero.
fn approval_doctor(read: Option<&Result<Value, String>>) -> Vec<String> {
    let report = match read {
        None => return vec!["approval_unmatched: not read; an admin key reads it".to_owned()],
        Some(Err(reason)) => return vec![format!("warn approval_unmatched: not read ({reason})")],
        Some(Ok(report)) => report,
    };
    let mut lines = Vec::new();
    if let Some(count) = report.get("promoted_unapproved") {
        lines.push(unmatched_line(count));
    }
    // The steering and guardrails in force on a non-person approval (ADR-0080
    // d16; #436): nothing at zero, nothing from a server before the count.
    if let Some(count) = report.get("instructions_without_person_approval") {
        match count.as_u64() {
            Some(0) => {}
            Some(n) => lines.push(format!(
                "warn instruction_approvals: {n} instruction (steering, guardrail or skill) version(s) in force were approved by a key that is not a person's; a person can review each again by superseding it (ADR-0080 d16)"
            )),
            None => lines.push(format!(
                "warn instruction_approvals: the server's instructions_without_person_approval is {count}, not a count"
            )),
        }
    }
    lines
}

/// The `approval_unmatched` line for the server's `promoted_unapproved`.
fn unmatched_line(count: &Value) -> String {
    let Some(unmatched) = count.as_u64() else {
        return format!(
            "warn approval_unmatched: the server's promoted_unapproved is {count}, not a count"
        );
    };
    if unmatched > 0 {
        format!(
            "warn approval_unmatched: {unmatched} served version(s) carry no approval, because migration 0044 found no verdict for their promotion in the journal; recall serves them with approver and approved_at null, and no command lists which they are; to clear one, supersede it with oml write --replaces, and it is no longer counted once a reviewer approves the successor; or leave it"
        )
    } else {
        "approval_unmatched: 0".to_owned()
    }
}

/// The unreviewable-instruction line (ADR-0080 d16; #435), from the admin's
/// queue read: one warn per workspace holding steering or guardrail versions
/// that no live person's reviewer key reads with reviewer, and nothing for a
/// workspace someone can approve in. Counts by path only. A server before the
/// counts names neither, and its items are skipped; a count that is no
/// unsigned integer is named, never read as zero.
fn instructions_unreviewable_doctor(read: Option<&Result<Value, String>>) -> Vec<String> {
    let queue = match read {
        None => {
            return vec!["instructions_unreviewable: not read; an admin key reads it".to_owned()];
        }
        Some(Err(reason)) => {
            return vec![format!(
                "warn instructions_unreviewable: not read ({reason})"
            )];
        }
        Some(Ok(queue)) => queue,
    };
    let mut lines = Vec::new();
    for item in queue["items"].as_array().into_iter().flatten() {
        let path = item["path"].as_str().unwrap_or("?");
        let (Some(held), Some(people)) =
            (item.get("held_instructions"), item.get("person_reviewers"))
        else {
            continue;
        };
        match (held.as_u64(), people.as_u64()) {
            (Some(0), Some(_)) | (Some(_), Some(1..)) => {}
            (Some(held), Some(0)) => lines.push(format!(
                "warn instructions_unreviewable: {path} holds {held} instruction (steering, guardrail or skill) version(s) and no person's reviewer key reads it; issue a reviewer key with --used-by person and a reviewer grant there, or approve in the console (review.person_required, instruction_needs_person)"
            )),
            _ => lines.push(format!(
                "warn instructions_unreviewable: the server's counts for {path} are {held} and {people}, not counts"
            )),
        }
    }
    lines
}

/// Doctor's `memory.withdrawn_in_queue` (ADR-0083 d13), from the same
/// admin rescan window as `approval_unmatched`: it fails when a withdrawn
/// version is still in a review queue, which the withdrawal's own
/// transaction rules out, so a count above zero names a store fault. A
/// server before the count does not report it, and the line is then
/// absent; a count that is no unsigned integer is named, never read as
/// zero.
fn withdrawn_queue_doctor(read: Option<&Result<Value, String>>) -> Vec<String> {
    let report = match read {
        None => {
            return vec!["memory.withdrawn_in_queue: not read; an admin key reads it".to_owned()];
        }
        Some(Err(reason)) => {
            return vec![format!(
                "warn memory.withdrawn_in_queue: not read ({reason})"
            )];
        }
        Some(Ok(report)) => report,
    };
    let Some(count) = report.get("withdrawn_in_queue") else {
        return Vec::new();
    };
    let Some(queued) = count.as_u64() else {
        return vec![format!(
            "warn memory.withdrawn_in_queue: the server's withdrawn_in_queue is {count}, not a count"
        )];
    };
    vec![if queued > 0 {
        format!(
            "fail memory.withdrawn_in_queue: {queued} withdrawn version(s) are still in a review queue, which a withdrawal's own transaction rules out; reviewers may be served them; report it with the server's logs"
        )
    } else {
        "memory.withdrawn_in_queue: 0".to_owned()
    }]
}

/// Doctor's public workspace lines (ADR-0098 d9), from the same admin
/// rescan window: `workspace.public` names each public workspace, since
/// when, by which act, whether it took effect under `sole_admin`, and its
/// served rows, the load it adds to every default recall of the tenant;
/// `workspace.public_ineligible` fails for a public workspace that is
/// `development` or inside a user root, which only a direct store edit
/// produces. A server before the flag reports neither, and the lines are
/// then absent.
fn public_doctor(read: Option<&Result<Value, String>>) -> Vec<String> {
    let Some(Ok(report)) = read else {
        return Vec::new();
    };
    let (Some(public), Some(ineligible)) = (
        report.get("public_workspaces").and_then(Value::as_array),
        report.get("public_ineligible").and_then(Value::as_array),
    ) else {
        return Vec::new();
    };
    let rows: u64 = public
        .iter()
        .filter_map(|w| w["served_rows"].as_u64())
        .sum();
    let mut lines = vec![format!(
        "workspace.public: {} public workspace(s), {rows} served row(s) every default recall of the tenant also searches",
        public.len()
    )];
    for w in public {
        lines.push(format!(
            "workspace.public: {} since {} (act {}{}), {} served row(s)",
            w["name"].as_str().unwrap_or("?"),
            w["since"].as_str().unwrap_or("?"),
            w["act_id"].as_str().unwrap_or("?"),
            if w["sole_admin"] == true {
                ", sole_admin"
            } else {
                ""
            },
            w["served_rows"].as_u64().unwrap_or(0)
        ));
    }
    lines.push(if ineligible.is_empty() {
        "workspace.public_ineligible: 0".to_owned()
    } else {
        format!(
            "fail workspace.public_ineligible: {} public workspace(s) are development or inside a user root, which no admin act allows; clear each with oml workspaces public clear <workspace>",
            ineligible.len()
        )
    });
    lines
}

/// Doctor's `workspace.flagged_creates` (ADR-0097 d31, the operator's
/// decision of 2026-11-27), an info line from the same admin rescan window:
/// the workspaces keys created in the shared tree under a grant carrying
/// `create`, per creating key. No account's `workspaces` meter counts them,
/// so a tenant agent key with the flag is bounded only by its request rate.
/// A server before the flag reports none, and the line is then absent.
fn flagged_creates_doctor(read: Option<&Result<Value, String>>) -> Vec<String> {
    let Some(Ok(report)) = read else {
        return Vec::new();
    };
    let Some(keys) = report.get("flagged_creates").and_then(Value::as_array) else {
        return Vec::new();
    };
    let total: u64 = keys.iter().filter_map(|k| k["workspaces"].as_u64()).sum();
    let mut lines = vec![format!(
        "workspace.flagged_creates: {total} workspace(s) created in the shared tree under a create grant, by {} key(s); no meter counts them, only each key's request rate bounds them",
        keys.len()
    )];
    for k in keys {
        lines.push(format!(
            "workspace.flagged_creates: key {} created {}",
            k["key_id"].as_str().unwrap_or("?"),
            k["workspaces"].as_u64().unwrap_or(0)
        ));
    }
    lines
}

/// Doctor's lifecycle lines (ADR-0089 d16, notes of 2026-11-18; ADR-0085
/// d16's pending links, the operator's decision of 2026-11-19), from the
/// same admin rescan window: `memory.held_successor_stranded` warns of held
/// versions whose target is no longer its claim's promoted tip, which no
/// verdict can approve (a demoted tip's successors among them), and
/// `lake.erased_key_live` of erased versions whose data key is still live,
/// which the erase's own transaction and the lake writer's fold of it rule
/// out. A server before a count does not report it, and its line is then
/// absent; a count that is no unsigned integer is named, never read as zero.
fn lifecycle_doctor(read: Option<&Result<Value, String>>) -> Vec<String> {
    const LINES: [(&str, &str, &str); 20] = [
        (
            "held_successors_stranded",
            "memory.held_successor_stranded",
            "held version(s) replace a version that is no longer their claim's promoted tip, so no verdict can approve them; their authors withdraw them or a reviewer rejects them",
        ),
        (
            "erased_keys_live",
            "lake.erased_key_live",
            "erased version(s) still have a live data key, which the erase destroys in its own transaction; the lake writer destroys it when it drains the erase, so check that it runs",
        ),
        (
            "investigations_open_long",
            "admin.investigation_open_long",
            "admin investigation(s) open for more than 7 days; the key that opened each closes it with oml admin investigate --close <id>",
        ),
        (
            "fixes_unconfirmed",
            "admin.fix_unconfirmed",
            "admin fix(es) no person has confirmed or reverted; a person's reviewer key lists them with oml review fixes",
        ),
        (
            "fixes_loosening_guardrail",
            "admin.fix_loosens_guardrail",
            "unconfirmed admin fix(es) took a served instruction out of service or rewrote one; a person confirms or reverts each with oml review fixes",
        ),
        (
            "fixes_without_person_reviewer",
            "admin.fix_without_person_reviewer",
            "unconfirmed admin fix(es) sit on a workspace where no person's reviewer key holds reviewer, so no one can confirm them; grant reviewer to a person there",
        ),
        (
            "fixes_admin_person_only",
            "admin.fix_admin_person_only",
            "unconfirmed admin fix(es) sit on a workspace where only the admin's own person holds reviewer; another person confirms them",
        ),
        (
            "fixes_reverted",
            "admin.fix_reverted",
            "admin fix(es) a person reverted; read each with oml admin fixes --all",
        ),
        (
            "key_links_pending",
            "key.link_pending",
            "key link(s) wait on their person's acceptance; each key acts for no one, and an admin key among them takes no fix, until that person accepts the link on the console's Keys page",
        ),
        (
            "web_emails_pending",
            "web.email_pending",
            "sign-in email(s) wait on their person's acceptance; each signs in to nothing until that person accepts it on the console's Keys page, or the person or an admin removes it",
        ),
        (
            "admin_acts_pending",
            "admin.act_pending_confirmation",
            "admin act(s) wait on a second admin person; each takes effect only once confirmed with oml admin confirm <act>, and expires as declined after 7 days",
        ),
        (
            "admin_erase_shreds_pending",
            "admin.erase_shred_pending",
            "erase(s) demoted their claim and keep its bytes until a second admin person confirms the shred with oml admin confirm <act>, or declines it",
        ),
        (
            "admin_acts_sole_admin",
            "admin.act_sole_admin",
            "admin act(s) took effect at once because the tenant has only ever had one admin person; a second admin person reviews them with oml admin acts --state effective",
        ),
        (
            "admin_acts_without_reason",
            "admin.act_without_reason",
            "admin act(s) carry no reason; send --reason with each admin act",
        ),
        (
            "admin_acts_expired",
            "admin.act_expired",
            "admin act(s) expired unconfirmed in the last 7 days and never took effect; take the act again if it is still wanted",
        ),
        (
            "admin_narrowing_burst",
            "admin.narrowing_burst",
            "narrowing acts by one admin person in the last 24 hours, past the 20 doctor flags; read them with oml admin acts --state effective",
        ),
        (
            "admin_acts_offensive",
            "admin.offensive_narrowing",
            "offensive narrowing act(s) in the last 24 hours: a person's key revoked or a reviewer person's key denied; read each with oml admin acts --state effective",
        ),
        (
            "admin_keys_suspended",
            "admin.key_suspended",
            "admin key(s) suspended while an act against their person is open; settling the act revokes them or lifts the suspension",
        ),
        (
            "admin_keys_unlinked_suspended",
            "admin.key_unlinked_suspended",
            "admin key(s) suspended as unlinked at the upgrade; each links itself with oml keys link-self --person <name> and its person accepts on the console's Keys page",
        ),
        (
            "admin_disputes_open",
            "admin.dispute_open",
            "act(s) against an admin open below three admin persons: both parties take no widening act until the operator settles it from the host",
        ),
    ];
    let report = match read {
        None => {
            return LINES
                .iter()
                .map(|(_, line, _)| format!("{line}: not read; an admin key reads it"))
                .collect();
        }
        Some(Err(reason)) => {
            return LINES
                .iter()
                .map(|(_, line, _)| format!("warn {line}: not read ({reason})"))
                .collect();
        }
        Some(Ok(report)) => report,
    };
    LINES
        .iter()
        .filter_map(|(field, line, what)| {
            let count = report.get(*field)?;
            Some(match count.as_u64() {
                None => format!("warn {line}: the server's {field} is {count}, not a count"),
                Some(0) => format!("{line}: 0"),
                // At most 20 narrowing acts by one person in a day are
                // ordinary: a placeholder (ADR-0097 d24).
                Some(n) if *field == "admin_narrowing_burst" && n <= 20 => format!("{line}: {n}"),
                // Critical: offensive narrowing is loud (ADR-0097 d24).
                Some(n) if *field == "admin_acts_offensive" || *field == "admin_disputes_open" => {
                    format!("fail {line}: {n} {what}")
                }
                Some(n) if *field == "admin_keys_unlinked_suspended" => {
                    let left = report
                        .get("admin_unlinked_days_left")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    format!("warn {line}: {n} {what}; {left} days left before revocation")
                }
                Some(n) if *field == "admin_erase_shreds_pending" => {
                    let oldest = report
                        .get("admin_erase_shred_oldest_seconds")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    format!("warn {line}: {n} {what}; the oldest is {oldest} s old")
                }
                Some(n) if *field == "admin_acts_pending" => {
                    let oldest = report
                        .get("admin_act_oldest_pending_seconds")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    format!("warn {line}: {n} {what}; the oldest is {oldest} s old")
                }
                Some(n) if *field == "fixes_unconfirmed" => {
                    let oldest = report
                        .get("fix_oldest_unconfirmed_seconds")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    format!("warn {line}: {n} {what}; the oldest is {oldest} s old")
                }
                Some(n) => format!("warn {line}: {n} {what}"),
            })
        })
        .collect()
}

/// The scan doctor lines (ADR-0065 d11): the rule set `/readyz` names, how
/// many rules it loaded and whether the personal-data rules are on; and,
/// where `OML_METRICS_URL` names the metrics listener, the findings since
/// start by action and the rules that fired most. Counts only: no finding
/// carries what it matched.
fn scan_doctor(ready: Option<&(u16, Value)>, scrape: Option<&Scrape>) -> Vec<String> {
    // No URL: doctor's url line names no_server, and nothing stands in for
    // the probe (ADR-0076 d8).
    let Some((_, body)) = ready else {
        return Vec::new();
    };
    let scan = &body["scan"];
    let Some(version) = scan["version"].as_str() else {
        return vec![
            "warn scan_ruleset: the server names no rule set; one before v0.6.0 scans for two test markers only"
                .to_owned(),
        ];
    };
    let rules = scan["rules"].as_u64().unwrap_or(0);
    let mut lines = vec![format!(
        "{version}, {rules} rules loaded, personal-data rules {}",
        if scan["pii"] == true {
            "on (hold)"
        } else {
            "off"
        }
    )];
    if rules == 0 {
        lines.push(format!(
            "warn scan_rules: {version} loaded no rules, so every write passes unscanned; this build is defective"
        ));
    }
    match scrape {
        None => lines.push(
            "findings not read: set OML_METRICS_URL to the lead's metrics listener".to_owned(),
        ),
        Some(scrape) => {
            let vetoes = scrape.sum("oml_scan_findings_total", &[("action", "veto")]);
            let holds = scrape.sum("oml_scan_findings_total", &[("action", "hold")]);
            let mut top: Vec<(String, u64)> = scrape
                .samples("oml_scan_findings_total", &[])
                .filter_map(|(set, n)| {
                    let rule = set
                        .split(',')
                        .find_map(|pair| pair.strip_prefix("rule=\""))?
                        .trim_end_matches('"');
                    Some((rule.to_owned(), n))
                })
                .collect();
            top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            let named = top
                .iter()
                .take(3)
                .map(|(rule, n)| format!("{rule} ×{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(if named.is_empty() {
                "findings since start: none".to_owned()
            } else {
                format!("findings since start: {vetoes} refused, {holds} held; most: {named}")
            });
        }
    }
    lines
}

/// The learned sparse doctor lines (ADR-0052 d15, d16), where a served
/// pipeline reads kind 2: the encoder `/readyz` names, or a warning that
/// none is loaded; and, read by an admin key, the tenant's versions still
/// waiting for the encoder and those whose encode failed.
fn sparse_doctor(body: &Value, enabled: &[&str], settings: Option<&Value>) -> Vec<String> {
    let sparse: Vec<&str> = enabled
        .iter()
        .copied()
        .filter(|p| matches!(*p, "sparse" | "bm25-sparse" | "bm25-sparse-rrf"))
        .collect();
    let h = settings.map(|s| &s["postings"]).filter(|h| h.is_object());
    let n = |field: &str| h.and_then(|h| h[field].as_u64()).unwrap_or(0);
    if sparse.is_empty() {
        // A backlog where no sparse pipeline is served has nothing to drain
        // it: another lead serving one is enqueueing, or this lead has not
        // restarted since one stopped (ADR-0052 d15).
        if n("sparse_backlog") > 0 {
            return vec![format!(
                "warn search_sparse_undrained: {} memory versions await a sparse encoder, but \
                 this lead serves no sparse pipeline; serve sparse or bm25-sparse on every lead, \
                 or restart leads without sparse to disable and clear the queue",
                n("sparse_backlog")
            )];
        }
        return Vec::new();
    }
    let mut lines = Vec::new();
    let Some(model) = body["search"]["sparse"]["model"].as_str() else {
        return vec![format!(
            "warn search_sparse_model: search.enabled lists {}, but /readyz reports no sparse \
             encoder. Place model files in search.sparse.model_dir (scripts/fetch_sparse_model.sh) \
             or set search.sparse.fetch = true, then restart",
            sparse.join(", ")
        )];
    };
    match h {
        Some(_) => lines.push(format!("sparse {model}, backlog {}", n("sparse_backlog"))),
        None => lines.push(format!("sparse {model}")),
    }
    if n("sparse_backlog") > 0 {
        lines.push(format!(
            "warn search_sparse_backlog: {} memory versions wait for the sparse encoder, so sparse lists miss them until it reaches them; bm25-postings still finds them; it drains on its own, oldest first (oml_sparse_backlog on the metrics listener)",
            n("sparse_backlog")
        ));
    }
    if n("sparse_parked") > 0 {
        lines.push(format!(
            "warn search_sparse_parked: {} memory versions failed to encode too often and are parked, so sparse lists never hold them until they are written again; search_sparse_queue.last_error names each reason (oml_sparse_encoded_total{{outcome=\"parked\"}})",
            n("sparse_parked")
        ));
    }
    if n("sparse_failing") > 0 {
        lines.push(format!(
            "warn search_sparse_failed: {} memory versions failed to encode and are retried after at most an hour, each parked after five failures; search_sparse_queue.last_error names each reason, and the lead's log event sparse_encode_failed counts them",
            n("sparse_failing")
        ));
    }
    lines
}

/// The share of `bm25-postings`'s recalls since the lead started that the
/// scan may answer before doctor warns (ADR-0052 open question 2).
const SCAN_SHARE_WARN_PERCENT: u64 = 5;

/// The fewest `bm25-postings` recalls since start over which doctor judges
/// the scan share: below it one recall would swing it.
const SCAN_SHARE_MIN_RECALLS: u64 = 20;

/// One scrape of the lead's metrics listener (ADR-0042 d11), which doctor
/// reads every traffic count from.
struct Scrape(String);

impl Scrape {
    /// `GET {base}/metrics`, or `None` when it does not answer.
    fn fetch(base: &str) -> Option<Self> {
        ureq::get(&format!("{base}/metrics"))
            .call()
            .ok()?
            .into_string()
            .ok()
            .map(Self)
    }

    /// The sum of every sample of `name` whose labels hold each of `labels`.
    fn sum(&self, name: &str, labels: &[(&str, &str)]) -> u64 {
        self.samples(name, labels).map(|(_, v)| v).sum()
    }

    /// Every sample of `name` whose labels hold each of `labels`, as its
    /// label text and integral value.
    fn samples<'a>(
        &'a self,
        name: &'a str,
        labels: &'a [(&'a str, &'a str)],
    ) -> impl Iterator<Item = (&'a str, u64)> + 'a {
        self.0.lines().filter_map(move |line| {
            let rest = line.strip_prefix(name)?;
            let (set, value) = match rest.strip_prefix('{') {
                Some(rest) => rest.split_once("} ")?,
                None => ("", rest.strip_prefix(' ')?),
            };
            if !labels
                .iter()
                .all(|(k, v)| set.split(',').any(|pair| pair == format!("{k}=\"{v}\"")))
            {
                return None;
            }
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a counter's or gauge's integral value"
            )]
            let value = value
                .split(' ')
                .next()
                .and_then(|v| v.parse::<f64>().ok())
                .map_or(0, |v| v.max(0.0) as u64);
            Some((set, value))
        })
    }
}

/// The valid-time doctor line (proposed ADR-0059 d12), read by an admin key
/// from `GET /v2/search`: how many served memories declare valid time and
/// how many have ended and are hidden from recall, under every pipeline,
/// since the expiry rule binds the default too; and, where a time pipeline
/// is enabled and none declares any, a warning that its `time` list is
/// always empty.
fn valid_time_doctor(enabled: &[&str], settings: Option<&Value>) -> Vec<String> {
    let mut lines = as_of_doctor(settings);
    lines.extend(valid_time_lines(enabled, settings));
    lines
}

/// The as-of read's doctor line (ADR-0059 d16): a warning naming the
/// promoted versions with no `promoted_at`, which an as-of read never serves,
/// serving the version before each in its place; nothing when there are none.
fn as_of_doctor(settings: Option<&Value>) -> Vec<String> {
    let untimed = settings
        .map(|s| s["valid_time"]["untimed"].as_u64().unwrap_or(0))
        .unwrap_or(0);
    if untimed == 0 {
        return Vec::new();
    }
    vec![format!(
        "warn as_of: {untimed} promoted versions have no promoted_at, so an as-of read never \
         serves them and serves the version each replaced in its place; they predate migration \
         0005's backfill"
    )]
}

fn valid_time_lines(enabled: &[&str], settings: Option<&Value>) -> Vec<String> {
    let timed: Vec<&str> = enabled
        .iter()
        .copied()
        .filter(|p| p.starts_with("bm25-postings-time"))
        .collect();
    let Some(v) = settings.map(|s| &s["valid_time"]).filter(|v| v.is_object()) else {
        return Vec::new();
    };
    let (declared, expired) = (
        v["declared"].as_u64().unwrap_or(0),
        v["expired"].as_u64().unwrap_or(0),
    );
    if declared == 0 && !timed.is_empty() {
        return vec![format!(
            "warn time: {} is enabled, but no served memory declares valid_from; time results \
             are empty and recall falls back to bm25-postings. Authors should set valid_from \
             and valid_until when writing",
            timed.join(", ")
        )];
    }
    if declared == 0 && expired == 0 {
        return Vec::new();
    }
    vec![format!(
        "time: {declared} memories declare valid time; {expired} ended and hidden from recall unless include_expired"
    )]
}

/// The `bm25-postings` doctor lines (ADR-0052 d12): from the metrics
/// listener's recall counts, a warning when the scan answered more than 5%
/// of its recalls since the lead started; and, read by an admin key from
/// `GET /v2/search`, one warning per fault in the tenant's postings, each
/// naming the repair, or one line saying they are complete.
fn postings_doctor(recall_counts: Option<(u64, u64)>, settings: Option<&Value>) -> Vec<String> {
    let mut lines = Vec::new();
    let (postings, scan) = recall_counts.unwrap_or((0, 0));
    let total = postings + scan;
    if total >= SCAN_SHARE_MIN_RECALLS && scan * 100 > SCAN_SHARE_WARN_PERCENT * total {
        #[allow(clippy::cast_precision_loss, reason = "a share of small counts")]
        let percent = scan as f64 * 100.0 / total as f64;
        lines.push(format!(
            "warn search: bm25-postings scanned {scan} of {total} recalls since startup \
             ({percent:.1}%, above {SCAN_SHARE_WARN_PERCENT}%); shared words across readable \
             memories make these recalls as costly as bm25"
        ));
    }
    let Some(h) = settings.map(|s| &s["postings"]).filter(|h| h.is_object()) else {
        return lines;
    };
    let n = |field: &str| h[field].as_u64().unwrap_or(0);
    let refill = "as the owner, in a transaction with oml.tenant_id set to the tenant, SELECT oml_search_postings_refill('<tenant id>') rebuilds them (postgres-schema.md, search postings)";
    if h["partition"] == Value::Bool(false) {
        lines.push(
            "warn search: this tenant has no postings partition, so every memory write fails; \
             restart the lead to create and fill it"
                .to_owned(),
        );
    }
    if n("unfilled") > 0 {
        lines.push(format!(
            "warn search: the postings fill is incomplete: {} memory versions have no postings; {refill}",
            n("unfilled")
        ));
    }
    if n("served_behind") > 0 {
        lines.push(format!(
            "warn search: {} served memories have no served postings, so bm25-postings cannot find them; a defect: {refill}, and report it",
            n("served_behind")
        ));
    }
    if n("workspace_drift") > 0 || n("term_drift") > 0 {
        lines.push(format!(
            "warn search: the postings counts disagree: {} workspace count{}, {} of {} sampled word counts; a defect, which ranks by wrong frequencies: {refill}, and report it",
            n("workspace_drift"),
            if n("workspace_drift") == 1 { "" } else { "s" },
            n("term_drift"),
            n("terms_checked")
        ));
    }
    // The similar check's keys (ADR-0050 d15, d17); a server before 0026
    // sends none of these fields, and none is read as a fault.
    let similar_refill = "as the owner, in a transaction with oml.tenant_id set to the tenant, SELECT oml_similar_keys_refill('<tenant id>') rebuilds them (postgres-schema.md, similar keys)";
    if h["similar_partition"] == Value::Bool(false) {
        lines.push(
            "warn similar: this tenant has no similar keys partition, so every memory write \
             fails; restart the lead to create and fill it"
                .to_owned(),
        );
    }
    if n("similar_unfilled") > 0 || n("similar_drift") > 0 {
        lines.push(format!(
            "warn similar: the similar keys disagree with the versions: {} without keys, {} with keys of another state; the write-time check misses or names the wrong rows until rebuilt: {similar_refill}, and report it",
            n("similar_unfilled"),
            n("similar_drift")
        ));
    }
    // A document version with no passage is searched whole (ADR-0081 d3,
    // d4); a server before 0059 sends no such field.
    if n("documents_uncut") > 0 {
        lines.push(format!(
            "warn search: {} document version{} have no passages, so recall finds none of them: one written before migration 0059 is cut when the lead restarts; one whose text holds no content line has nothing to find",
            n("documents_uncut"),
            if n("documents_uncut") == 1 { "" } else { "s" },
        ));
    }
    let faulty = h["partition"] != Value::Bool(true)
        || h["similar_partition"] == Value::Bool(false)
        || n("similar_unfilled") > 0
        || n("similar_drift") > 0
        || n("unfilled") > 0
        || n("served_behind") > 0
        || n("workspace_drift") > 0
        || n("term_drift") > 0;
    if !faulty {
        lines.push(format!(
            "postings complete; counts agree ({} sampled word counts)",
            n("terms_checked")
        ));
    }
    lines
}

/// The web pages' doctor lines (ADR-0045 d18): off when `/readyz` names no
/// provider; a warning per provider whose exchanges failed since the lead
/// started; and, read by an admin key, a warning when no entry of the
/// tenant's list opens the admin page.
fn web_doctor(
    ready: Option<&(u16, Value)>,
    admin_emails: Option<(usize, Vec<String>)>,
    scrape: Option<&Scrape>,
) -> Vec<String> {
    let Some((_, body)) = ready else {
        return Vec::new();
    };
    let Some(providers) = body["web"]["providers"].as_array() else {
        return vec!["off (no sign-in provider, or no oauth.issuer)".to_owned()];
    };
    let providers: Vec<&str> = providers.iter().filter_map(Value::as_str).collect();
    let mut lines = vec![format!("on ({})", providers.join(", "))];
    // A lead before ADR-0070 names no client identity; one that names
    // `none` refuses every capacity credential oml-web presents (d2).
    if body["web_client"] == "none" {
        lines.push(
            "warn web: no web client identity set; oml-web cannot reach the API (set web.client_secret on the lead and on oml-web)"
                .to_owned(),
        );
    }
    // The failed exchanges are the metrics listener's (ADR-0054 d1).
    for provider in &providers {
        let Some(scrape) = scrape else { break };
        let label = [("provider", *provider)];
        let reasons: Vec<String> = scrape
            .samples("oml_web_exchanges_failed_total", &label)
            .filter(|(_, n)| *n > 0)
            .map(|(set, n)| {
                let reason = set
                    .split(',')
                    .find_map(|pair| pair.strip_prefix("reason="))
                    .unwrap_or("\"unknown\"")
                    .trim_matches('"');
                format!("{n} {reason}")
            })
            .collect();
        let failed = scrape.sum("oml_web_exchanges_failed_total", &label);
        if failed > 0 {
            lines.push(format!(
                "warn web_provider_{provider}: {failed} failed exchanges since start ({})",
                reasons.join(", ")
            ));
        }
    }
    if let Some((admins, keyless)) = admin_emails {
        if admins == 0 {
            lines.push(
                "warn web: no admin email; add one with oml web allow <email> --user <user> --admin"
                    .to_owned(),
            );
        }
        for user in keyless {
            lines.push(format!(
                "warn web: {user} has no session key; it is created at {user}'s next sign-in"
            ));
        }
    }
    lines
}

/// Usage metering's doctor lines (ADR-0071 d3, d5, d8), from
/// `GET /v2/admin/usage/drift` read by an admin key: the accounts whose
/// counters differ from their rows, and the accounts on a profile the
/// configuration does not declare; nothing when neither holds, and
/// nothing for a key that is no admin's, which does not read it. A read
/// that fails is a line of its own: the counters are then unchecked.
/// The journal's former names, as `/readyz`'s `former_names` check reports
/// the lead's startup scan (ADR-0091 d4): `fail` names
/// `journal_unmapped_former_name` and where the lead's log lists each event;
/// nothing from a lead that does not report the check.
fn former_names_doctor(ready: Option<&(u16, Value)>) -> Option<String> {
    let (_, body) = ready?;
    match body["checks"]["former_names"].as_str()? {
        "ok" => Some(
            "ok journal_unmapped_former_name: every stored event reads in the current names"
                .to_owned(),
        ),
        _ => Some(
            "fail journal_unmapped_former_name: the journal holds a former name the former-name \
             map does not cover; the lead's start log names each tenant, seq and event \
             (ADR-0091 d4)"
                .to_owned(),
        ),
    }
}

fn usage_doctor(report: Option<&Result<Value, String>>) -> Vec<String> {
    let r = match report {
        None => return Vec::new(),
        Some(Err(reason)) => {
            return vec![format!(
                "warn usage: the drift report was not read ({reason}); the counters are unchecked against their rows"
            )];
        }
        Some(Ok(r)) => r,
    };
    let mut lines = Vec::new();
    let drift = r["drift"].as_array().map(Vec::as_slice).unwrap_or_default();
    if !drift.is_empty() {
        let accounts: std::collections::BTreeSet<&str> =
            drift.iter().filter_map(|d| d["account"].as_str()).collect();
        let each: Vec<String> = drift
            .iter()
            .map(|d| {
                format!(
                    "{} {} counter {} rows {}",
                    d["user"].as_str().or(d["account"].as_str()).unwrap_or("?"),
                    d["meter"].as_str().unwrap_or("?"),
                    d["counter"],
                    d["rows"]
                )
            })
            .collect();
        lines.push(format!(
            "warn usage: {} accounts drift from their rows ({})",
            accounts.len(),
            each.join(", ")
        ));
    }
    let mut undeclared: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for u in r["undeclared_profiles"].as_array().into_iter().flatten() {
        *undeclared
            .entry(u["profile"].as_str().unwrap_or("?"))
            .or_default() += 1;
    }
    for (profile, n) in undeclared {
        lines.push(format!(
            "warn usage: {n} accounts on profile {profile}, which the configuration does not declare; their writes are refused limit.quota until an admin reassigns them or declares it"
        ));
    }
    lines
}

/// Open sign-up's doctor lines (ADR-0068 d5, d6, d9, d11), from
/// `GET /v2/web/users` read by an admin key: the people at or above 80% of
/// a quota, the pending deletion requests, and an open mode whose sign-ups
/// share one address bucket because no proxy is trusted.
fn signup_doctor(overview: Option<&Value>) -> Vec<String> {
    let Some(o) = overview else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if o["mode"] == "open" && o["address_limit_shared"] == true {
        lines.push(
            "warn signup: open with no server.trusted_proxies; every sign-up shares one address limit"
                .to_owned(),
        );
    }
    let mut near: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let mut people = 0;
    for u in o["users"].as_array().into_iter().flatten() {
        if u["under_quotas"] != true || u["disabled"] == true {
            continue;
        }
        let mut hit = false;
        // A null limit is unlimited and never near (ADR-0071 d5).
        for (quota, used) in [
            ("memories", "memories"),
            ("text_bytes", "text_bytes"),
            ("writes_per_month", "writes"),
            ("keys", "keys"),
            ("connections", "connections"),
        ] {
            let used = u["usage"][used].as_u64().unwrap_or(0);
            let Some(limit) = u["quotas"][quota].as_u64() else {
                continue;
            };
            if limit > 0 && used.saturating_mul(10) >= limit.saturating_mul(8) {
                *near.entry(quota).or_default() += 1;
                hit = true;
            }
        }
        people += usize::from(hit);
    }
    if people > 0 {
        let by: Vec<String> = near.iter().map(|(q, n)| format!("{q}: {n}")).collect();
        lines.push(format!(
            "warn quota: {people} users at or above 80% of a quota ({})",
            by.join(", ")
        ));
    }
    // An admin key whose row names a person's root: no verb makes one, and
    // the person's disable refuses until it is gone (ADR-0068 d8, d11; #277).
    for u in o["users"].as_array().into_iter().flatten() {
        let user = u["user"].as_str().unwrap_or("?");
        for key in u["admin_keys"].as_array().into_iter().flatten() {
            lines.push(format!(
                "warn keys: admin key {} names the user root of {user}, which no verb does; disabling {user} is refused until the row's user_root_id is cleared",
                key.as_str().unwrap_or("?")
            ));
        }
    }
    if let Some(pending) = o["deletions"].as_array().filter(|d| !d.is_empty()) {
        lines.push(format!(
            "warn signup: {} deletion requests pending, oldest requested {}; close each on /web/admin/users",
            pending.len(),
            pending[0]["requested_at"].as_str().unwrap_or("?")
        ));
    }
    lines
}

/// Source anchors (ADR-0057 d17): a server before v0.5.8, which accepts no
/// anchor; the served versions of the key's default workspace that read
/// stale, as the server counts them; and the claim edges there waiting for
/// a second key.
fn sources_doctor(
    server: Option<&str>,
    default_node: Option<&Value>,
    waiting_edges: Option<usize>,
) -> Vec<String> {
    let mut lines = Vec::new();
    // A client of the same build as the server never warns: the check is
    // for a client that sends anchors to an older server.
    if let Some(version) = server
        && before_anchors(version)
        && !before_anchors(env!("CARGO_PKG_VERSION"))
    {
        lines.push(format!(
            "warn sources_unsupported: the server is {version}, before 0.5.8: it accepts no anchor, so --anchor, the anchor filters and oml sync are refused; upgrade the server"
        ));
    }
    if let Some(line) = rust_hashers_line(server, env!("CARGO_PKG_VERSION")) {
        lines.push(line);
    }
    if let Some(node) = default_node
        && let Some(stale) = node["stale_versions"].as_u64()
    {
        let path = node["path"].as_str().unwrap_or("the default workspace");
        if stale > 0 {
            lines.push(format!(
                "warn sources_stale: {stale} served version(s) in {path} read stale: their source changed or went away since they were reviewed, until a successor is reviewed"
            ));
        } else {
            lines.push(format!("no served version in {path} reads stale"));
        }
    }
    if let Some(waiting) = waiting_edges
        && waiting > 0
    {
        lines.push(format!(
            "{waiting} claim edge(s) in the default workspace wait for a second key to ratify them"
        ));
    }
    lines
}

/// A server before 0.6.1 refuses the hashers `oml-rustdoc/1` and
/// `oml-crate/1`, so `oml sync --adapter rust` is refused by it
/// (ADR-0061 d7, d15). Said only to a client that could send them.
fn rust_hashers_line(server: Option<&str>, client: &str) -> Option<String> {
    let version = server?;
    (before_version(version, (0, 6, 1)) && !before_version(client, (0, 6, 1))).then(|| {
        format!(
            "warn sources_rust_unsupported: the server is {version}, before 0.6.1: it refuses the hashers oml-rustdoc/1 and oml-crate/1, so oml sync --adapter rust is refused; upgrade the server"
        )
    })
}

/// Whether a server version is before 0.5.8, the first to accept anchors.
fn before_anchors(version: &str) -> bool {
    before_version(version, (0, 5, 8))
}

/// Whether `version` is before `first`; an unparsable version is not.
fn before_version(version: &str, first: (u64, u64, u64)) -> bool {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let parts: Vec<u64> = core.split('.').filter_map(|p| p.parse().ok()).collect();
    match parts.as_slice() {
        [major, minor, patch, ..] => (*major, *minor, *patch) < first,
        _ => false,
    }
}

/// The server's version, as `/readyz` names its build, ready or not
/// (http-api.md "Health"); `None` when the probe did not run or names none.
fn server_version(ready: Option<&(u16, Value)>) -> Option<String> {
    ready
        .and_then(|(_, body)| body["build"]["version"].as_str())
        .map(ToOwned::to_owned)
}

/// A failed call as doctor reports it: the status and code of a refusal, or
/// the local reason, which never carries a key.
fn refusal(e: &CliError) -> String {
    match e {
        CliError::Refused { status, code, .. } => format!("{status} {code}"),
        CliError::Local(why) | CliError::Unanswered(why) => why.clone(),
    }
}

fn verdict(ok: bool) -> &'static str {
    if ok { "ok" } else { "fail" }
}

#[cfg(test)]
mod tests {
    #[test]
    fn doctor_names_document_versions_with_no_passages() {
        let settings = serde_json::json!({"postings": {
            "partition": true, "similar_partition": true, "terms_checked": 4,
            "documents_uncut": 2,
        }});
        let lines = super::postings_doctor(None, Some(&settings));
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("warn search: 2 document versions have no passages")),
            "{lines:?}"
        );
        let clean = serde_json::json!({"postings": {
            "partition": true, "similar_partition": true, "terms_checked": 4,
            "documents_uncut": 0,
        }});
        let lines = super::postings_doctor(None, Some(&clean));
        assert!(
            !lines.iter().any(|l| l.contains("no passages")),
            "{lines:?}"
        );
    }

    use super::*;

    #[test]
    fn a_grant_takes_node_and_create_after_its_role_and_a_new_workspace_is_parent_slash_name() {
        let (node, role) = parse_grant("crew:submitter:create").expect("a grant");
        assert_eq!(
            grants_json(&[(node, role)]),
            [json!({ "workspace": "crew", "role": "submitter", "create": true })]
        );
        let both = parse_grant("crew:reader:create:node").expect("a grant");
        assert_eq!(
            grants_json(&[both]),
            [json!({ "workspace": "crew", "role": "reader", "covers": "node", "create": true })]
        );
        assert!(parse_grant("crew:reader:create:create").is_err());
        assert!(parse_grant("crew:reader:everything").is_err());
        assert_eq!(
            parse_new_workspace("universe.crew/alpha"),
            Ok(("universe.crew".to_owned(), "alpha".to_owned()))
        );
        assert!(parse_new_workspace("alpha").is_err());
        assert!(parse_new_workspace("crew/").is_err());
    }

    #[test]
    fn doctor_warns_a_reviewer_key_that_is_not_a_persons_that_instructions_need_a_person() {
        let agent =
            review_doctor(Some(&json!({"role": "reviewer", "used_by": "agent"}))).expect("a line");
        assert!(
            agent.starts_with("warn bulk review: this key is used by agent"),
            "{agent}"
        );
        assert!(
            agent.contains("review.person_required, instruction_needs_person"),
            "{agent}"
        );
        let crew = review_doctor(Some(&json!({
            "role": "reviewer", "used_by": "person", "crew": {"name": "ops"}, "profile": null,
        })))
        .expect("a line");
        assert!(crew.starts_with("warn instructions:"), "{crew}");
        let person = review_doctor(Some(&json!({
            "role": "reviewer", "used_by": "person", "crew": null, "profile": null,
        })))
        .expect("a line");
        assert!(person.starts_with("ok bulk review:"), "{person}");
        let service = review_doctor(Some(&json!({"role": "reviewer", "used_by": "service"})))
            .expect("a line");
        assert!(
            service.starts_with("warn review: this key is used by service, so it reviews nothing"),
            "{service}"
        );
        assert!(!service.contains("person_required"), "{service}");
        assert_eq!(review_doctor(Some(&json!({"role": "contributor"}))), None);
    }

    #[test]
    fn doctor_names_each_node_of_a_user_root_that_is_not_development() {
        let nodes = vec![
            json!({"name": "universe", "path": "universe", "status": "protected", "user_root": false}),
            json!({"name": "alice", "path": "universe.alice", "status": "protected", "user_root": true}),
            json!({"name": "proj", "path": "universe.alice.proj", "status": "development", "user_root": false}),
            json!({"name": "old", "path": "universe.alice.old", "status": "frozen", "user_root": false}),
            json!({"name": "alicea", "path": "universe.alicea", "status": "protected", "user_root": false}),
            json!({"name": "team", "path": "universe.team", "status": "frozen", "user_root": false}),
        ];
        let lines = user_root_ladder_doctor(&nodes, None);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].starts_with(
                "warn user_root_status: universe.alice is protected, inside the root of user alice"
            ),
            "{lines:?}"
        );
        assert!(
            lines[1].starts_with("warn user_root_status: universe.alice.old is frozen"),
            "{lines:?}"
        );
        let settled = [
            json!({"name": "alice", "path": "universe.alice", "status": "development", "user_root": true}),
            nodes[2].clone(),
            nodes[5].clone(),
        ];
        assert!(user_root_ladder_doctor(&settled, None).is_empty());
    }

    #[test]
    fn doctor_warns_when_the_tree_read_stops_before_its_last_page() {
        let nodes = [
            json!({"name": "alice", "path": "universe.alice", "status": "development", "user_root": true}),
        ];
        let lines = user_root_ladder_doctor(&nodes, Some("HTTP 503"));
        assert_eq!(
            lines,
            [
                "warn user_root_status: tree read incomplete after 1 node(s) (HTTP 503), so a \
              user-root node off development may be unnamed"
            ],
        );
    }

    #[test]
    fn printed_shell_commands_quote_each_path_as_one_word() {
        assert_eq!(shell_word("/tmp/my oml"), "'/tmp/my oml'");
        assert_eq!(shell_word("/tmp/it's"), "'/tmp/it'\\''s'");
    }

    #[test]
    fn the_formats_doctor_names_the_media_types_and_the_adapters_with_their_versions() {
        let ready = (
            200,
            json!({ "formats": {
                "enabled": ["text/plain", "text/vnd.oml.mermaid"],
                "adapters": ["identity/1", "mermaid/1"],
            }}),
        );
        assert_eq!(
            formats_doctor(Some(&ready)).as_deref(),
            Some("2 media types enabled; adapters identity/1, mermaid/1")
        );
        assert!(
            formats_doctor(Some(&(200, json!({}))))
                .expect("a line")
                .starts_with("not reported")
        );
        assert_eq!(formats_doctor(None), None);
    }

    #[test]
    fn the_doctor_warns_a_client_that_could_send_rust_hashers_when_the_server_refuses_them() {
        let line = rust_hashers_line(Some("0.6.0"), "0.6.1").unwrap();
        assert!(
            line.starts_with("warn sources_rust_unsupported: the server is 0.6.0"),
            "{line}"
        );
        assert_eq!(rust_hashers_line(Some("0.6.1"), "0.6.1"), None);
        assert_eq!(rust_hashers_line(Some("0.6.0"), "0.6.0"), None);
        assert_eq!(rust_hashers_line(None, "0.6.1"), None);
    }

    #[test]
    fn the_search_doctor_warns_that_a_recall_naming_no_pipeline_runs_the_deployments_default_while_the_tenant_default_is_not_served()
     {
        let ready = (
            200,
            json!({"search": {"enabled": ["keyword-any", "bm25"], "default": "bm25"}}),
        );
        let unserved = json!({
            "enabled": ["keyword-any", "bm25"], "deployment_default": "bm25",
            "tenant_default": "bm25-sparse", "effective": "bm25",
            "tenant_default_served": false,
        });
        let lines = search_doctor(Some(&ready), Some(&unserved), None);
        let line = lines
            .iter()
            .find(|l| l.starts_with("warn search_tenant_default:"))
            .unwrap_or_else(|| panic!("{lines:?}"));
        assert_eq!(
            line,
            "warn search_tenant_default: this tenant's default bm25-sparse is not served here, so a recall naming no pipeline runs bm25, marked search.fallback tenant_default_unserved; oml search set-default <pipeline>, or serve bm25-sparse"
        );
        let served = json!({
            "enabled": ["keyword-any", "bm25"], "deployment_default": "bm25",
            "tenant_default": "keyword-any", "effective": "keyword-any",
            "tenant_default_served": true,
        });
        let lines = search_doctor(Some(&ready), Some(&served), None);
        assert!(
            !lines.iter().any(|l| l.contains("search_tenant_default")),
            "{lines:?}"
        );
    }

    #[test]
    fn the_search_doctor_names_the_reranker_and_warns_on_skips_read_from_the_metrics_listener() {
        let ready = (
            200,
            json!({"search": {
                "enabled": ["bm25", "bm25-rerank"], "default": "bm25",
                "rerank": {"model": "ms-marco-MiniLM-L6-v2", "depth": 20, "floor": null},
            }}),
        );
        let scrape = Scrape(
            "oml_reranks_total{outcome=\"applied\",reason=\"none\"} 9\n\
             oml_reranks_total{outcome=\"skipped\",reason=\"busy\"} 3\n\
             oml_reranks_total{outcome=\"skipped\",reason=\"failed\"} 1\n"
                .to_owned(),
        );
        let lines = search_doctor(Some(&ready), None, Some(&scrape));
        assert_eq!(
            lines[1],
            "rerank ms-marco-MiniLM-L6-v2, depth 20, floor off"
        );
        assert!(
            lines[2].starts_with("warn search_rerank: 3 reranks skipped since start (3 busy)"),
            "{lines:?}"
        );
        assert!(
            lines[2].contains("search.rerank.max_concurrent"),
            "{lines:?}"
        );
        assert!(
            lines[3].starts_with("warn search_rerank_failed: 1 rerank failed since start"),
            "{lines:?}"
        );
        assert!(
            lines[3].contains("rerank_failed") && !lines[3].contains("max_concurrent"),
            "a failure names the log event, not the cap: {lines:?}"
        );
        // Without the metrics listener doctor says where the skips are,
        // never that there are none.
        let unread = search_doctor(Some(&ready), None, None);
        assert!(unread[2].contains("OML_METRICS_URL"), "{unread:?}");
        let floored = (
            200,
            json!({"search": {
                "enabled": ["bm25-rerank"], "default": "bm25-rerank",
                "rerank": {"model": "ms-marco-MiniLM-L6-v2", "depth": 10, "floor": 0.005},
            }}),
        );
        assert_eq!(
            search_doctor(Some(&floored), None, None)[1],
            "rerank ms-marco-MiniLM-L6-v2, depth 10, floor 0.005"
        );
        let quiet = (
            200,
            json!({"search": {"enabled": ["bm25"], "default": "bm25", "rerank": null}}),
        );
        assert_eq!(
            search_doctor(Some(&quiet), None, Some(&Scrape(String::new()))).len(),
            1,
            "no reranker, no line"
        );
    }

    #[test]
    fn the_handlers_doctor_warns_on_a_panic_the_guard_answered() {
        let panicked = Scrape("oml_handler_panics_total 2\n".to_owned());
        let lines = handlers_doctor(Some(&panicked));
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].starts_with("warn handler_panic: 2 request(s)"),
            "{lines:?}"
        );
        assert_eq!(
            handlers_doctor(Some(&Scrape("oml_handler_panics_total 0\n".to_owned()))),
            vec!["no handler panicked since start".to_owned()]
        );
        assert!(handlers_doctor(None)[0].contains("OML_METRICS_URL"));
    }

    #[test]
    fn the_handlers_doctor_warns_on_an_act_whose_idempotency_claim_was_left_unsettled() {
        let lines = handlers_doctor(Some(&Scrape(
            "oml_handler_panics_total 0\noml_idempotency_unsettled_total 3\n".to_owned(),
        )));
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[1].starts_with("warn idempotency_unsettled: 3 claimed act(s)"),
            "{lines:?}"
        );
        assert_eq!(
            handlers_doctor(Some(&Scrape(
                "oml_handler_panics_total 0\noml_idempotency_unsettled_total 0\n".to_owned()
            ))),
            vec!["no handler panicked since start".to_owned()],
            "no unsettled claim, no line"
        );
    }

    #[test]
    fn the_telemetry_doctor_names_drops_the_backlog_an_overdue_purge_and_the_lag() {
        let scrape = Scrape(
            "oml_recall_events_total{outcome=\"written\"} 90\n\
             oml_recall_events_total{outcome=\"dropped_no_partition\"} 10\n\
             oml_telemetry_queue_depth 6000\n\
             oml_telemetry_purge_overdue 2\n\
             oml_labeller_lag_seconds 900.0\n"
                .to_owned(),
        );
        let lines = telemetry_doctor(Some(&scrape));
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert!(lines[0].starts_with("warn telemetry_dropped: 10 of 100 recall events dropped since start (10 no_partition)"), "{lines:?}");
        assert!(
            lines[1].starts_with("warn telemetry_backlog: 6000"),
            "{lines:?}"
        );
        assert!(lines[2].starts_with("warn telemetry_purge: 2"), "{lines:?}");
        assert!(
            lines[3]
                .starts_with("warn telemetry_labeller: the labeller's last pass ended 900 s ago"),
            "{lines:?}"
        );
        let healthy = Scrape(
            "oml_recall_events_total{outcome=\"written\"} 7\n\
             oml_telemetry_queue_depth 0\n"
                .to_owned(),
        );
        assert_eq!(
            telemetry_doctor(Some(&healthy)),
            vec!["on: 7 recall events written since start, none dropped past 1%".to_owned()]
        );
        assert!(telemetry_doctor(None)[0].contains("OML_METRICS_URL"));
    }

    #[test]
    fn the_scan_doctor_names_the_rule_set_the_pii_setting_and_the_findings_by_rule() {
        let ready = (
            200,
            json!({"scan": {"version": "oml-scan/1", "rules": 41, "pii": false}}),
        );
        let scrape = Scrape(
            "oml_scan_findings_total{class=\"secret.api_token\",rule=\"github-token\",action=\"veto\"} 3\n\
             oml_scan_findings_total{class=\"injection.override\",rule=\"ignore-instructions\",action=\"hold\"} 5\n"
                .to_owned(),
        );
        assert_eq!(
            scan_doctor(Some(&ready), Some(&scrape)),
            vec![
                "oml-scan/1, 41 rules loaded, personal-data rules off".to_owned(),
                "findings since start: 3 refused, 5 held; most: ignore-instructions ×5, github-token ×3"
                    .to_owned(),
            ]
        );
        assert!(scan_doctor(Some(&ready), None)[1].contains("OML_METRICS_URL"));
        let none = (
            200,
            json!({"scan": {"version": "oml-scan/1", "rules": 0, "pii": true}}),
        );
        let lines = scan_doctor(Some(&none), Some(&Scrape(String::new())));
        assert!(
            lines[0].ends_with("personal-data rules on (hold)"),
            "{lines:?}"
        );
        assert!(lines[1].starts_with("warn scan_rules:"), "{lines:?}");
        assert_eq!(lines[2], "findings since start: none");
        let old = (200, json!({"status": "ok"}));
        assert!(scan_doctor(Some(&old), None)[0].starts_with("warn scan_ruleset:"));
    }

    /// ADR-0074 d23: a held version a server older than migration 0043
    /// wrote has no findings list, and doctor names how many and the fix.
    #[test]
    fn the_scan_doctor_warns_of_held_versions_whose_finding_the_findings_list_lacks() {
        let report = json!({
            "scanner_version": "oml-scan/1.2", "held_unrescanned": 0, "rescan_due": 0,
            "findings_unlisted": 2,
        });
        assert_eq!(
            scan_backlog_doctor(Some(&Ok(report)))[2],
            "warn scan_findings_unlisted: 2 held version(s) carry a scan finding the findings list lacks, written by a server older than migration 0043; the review card shows only the stored summary for them; run oml admin rescan --record-held"
        );
        let clear = json!({
            "scanner_version": "oml-scan/1.2", "held_unrescanned": 0, "rescan_due": 0,
            "findings_unlisted": 0,
        });
        assert_eq!(
            scan_backlog_doctor(Some(&Ok(clear)))[2],
            "scan_findings_unlisted: 0"
        );
    }

    /// ADR-0074 d18: the dry run's rates are aggregates per class, per
    /// origin and per class within an origin, a version counts once per
    /// cell, a count under the floor prints as `<5` with no rate, the
    /// header says the output is the operator's, and no id or text is
    /// printed.
    #[test]
    fn the_rescan_rates_print_counts_under_five_as_under_five_for_the_operator_with_no_id_or_text()
    {
        let id = |n: u128| format!("{:032x}", n);
        let mut findings = Vec::new();
        // Six chat versions held for one rule, one of them twice.
        for n in 0..6 {
            findings.push(json!({"artifact_id": id(n), "version": 1, "class": "injection.override", "rule": "ignore-instructions", "action": "hold", "origin": "chat", "at": "/body line 1"}));
        }
        findings.push(json!({"artifact_id": id(0), "version": 1, "class": "injection.override", "rule": "new-instructions", "action": "hold", "origin": "chat", "at": "/body line 2"}));
        // One web version holding a secret: a cell of one.
        findings.push(json!({"artifact_id": id(99), "version": 2, "class": "secret.api_token", "rule": "github-token", "action": "veto", "origin": "web", "at": "/body line 1"}));
        let by_origin =
            std::collections::BTreeMap::from([("chat".to_owned(), 10_u64), ("web".to_owned(), 1)]);
        let report = rescan_rates(Some("oml-scan/1.2"), 11, &by_origin, &findings);
        assert_eq!(
            report["classes"],
            json!([
                {"class": "injection.override", "verdict": "hold", "versions": 6, "rate": 6.0 / 11.0},
                {"class": "secret.api_token", "verdict": "refuse", "versions": "<5", "rate": null},
            ])
        );
        assert_eq!(
            report["origins"],
            json!([
                {"origin": "chat", "scanned": 10, "refuse": 0, "refuse_rate": 0.0, "hold": 6, "hold_rate": 0.6},
                {"origin": "web", "scanned": 1, "refuse": "<5", "refuse_rate": null, "hold": 0, "hold_rate": 0.0},
            ])
        );
        assert_eq!(
            report["cells"][1],
            json!({"class": "secret.api_token", "origin": "web", "verdict": "refuse", "versions": "<5", "rate": null})
        );
        let lines = rates_lines(&report);
        assert!(lines[0].contains("counts under 5 print as <5"), "{lines:?}");
        assert!(lines[0].contains("not for publication"), "{lines:?}");
        let text = format!("{report}\n{}", lines.join("\n"));
        assert!(
            !text.contains("refuse 1 ") && !text.contains("\"refuse\":1"),
            "{text}"
        );
        assert!(
            lines.iter().all(|l| !l.contains(" 1 version(s)")),
            "a count of one is never printed: {lines:?}"
        );
        for n in [0_u128, 5, 99] {
            assert!(!text.contains(&id(n)), "{text}");
        }
        assert!(!text.contains("/body"), "{text}");
    }

    #[test]
    fn the_scan_doctor_warns_of_held_versions_and_versions_an_older_rule_set_scanned() {
        let due = json!({
            "scanner_version": "oml-scan/1.2", "held_unrescanned": 4, "rescan_due": 9,
        });
        assert_eq!(
            scan_backlog_doctor(Some(&Ok(due))),
            vec![
                "warn scan_held_unrescanned: 4 held version(s) were scanned under an older rule set than oml-scan/1.2, and stay their authors' alone in in-progress workspaces; run oml admin rescan --record-held".to_owned(),
                "warn scan_rescan_due: 9 stored version(s) were scanned under an older rule set than oml-scan/1.2; oml admin rescan reports what it finds in them".to_owned(),
            ]
        );
        let clear = json!({
            "scanner_version": "oml-scan/1.2", "held_unrescanned": 0, "rescan_due": 0,
        });
        assert_eq!(
            scan_backlog_doctor(Some(&Ok(clear))),
            vec![
                "scan_held_unrescanned: 0".to_owned(),
                "scan_rescan_due: 0".to_owned(),
            ]
        );
        assert_eq!(
            scan_backlog_doctor(None),
            vec![
                "scan_held_unrescanned, scan_rescan_due: not read; an admin key reads them"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn the_scan_doctor_names_why_an_admins_failed_read_is_not_read() {
        // The admin's read failed: a warn naming why, not "an admin key
        // reads them", which this key is.
        assert_eq!(
            scan_backlog_doctor(Some(&Err("503 unavailable".to_owned()))),
            vec![
                "warn scan_held_unrescanned, scan_rescan_due: not read (503 unavailable)"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn the_approval_doctor_names_the_count_of_promoted_versions_with_no_approval() {
        let some = json!({ "scanner_version": "oml-scan/1.2", "promoted_unapproved": 3 });
        assert_eq!(
            approval_doctor(Some(&Ok(some))),
            vec!["warn approval_unmatched: 3 served version(s) carry no approval, because migration 0044 found no verdict for their promotion in the journal; recall serves them with approver and approved_at null, and no command lists which they are; to clear one, supersede it with oml write --replaces, and it is no longer counted once a reviewer approves the successor; or leave it".to_owned()]
        );
        let none = json!({ "scanner_version": "oml-scan/1.2", "promoted_unapproved": 0 });
        assert_eq!(
            approval_doctor(Some(&Ok(none))),
            vec!["approval_unmatched: 0".to_owned()]
        );
        // A server before the count says nothing, and a key that cannot read
        // the window says so.
        assert!(
            approval_doctor(Some(&Ok(json!({ "scanner_version": "oml-scan/1.2" })))).is_empty()
        );
        assert_eq!(
            approval_doctor(None),
            vec!["approval_unmatched: not read; an admin key reads it".to_owned()]
        );
    }

    #[test]
    fn the_exports_doctor_names_export_refusals_by_code() {
        let lines = exports_doctor(Some(&Scrape(
            "oml_exports_total 3\n\
             oml_export_refusals_total{code=\"export.too_large\"} 1\n\
             oml_export_refusals_total{code=\"export.busy\"} 2\n"
                .to_owned(),
        )));
        assert_eq!(
            lines.get(2).map(String::as_str),
            Some("export_refusals since start: export.busy 2, export.too_large 1"),
            "{lines:?}"
        );
        let quiet = exports_doctor(Some(&Scrape("oml_exports_total 3\n".to_owned())));
        assert_eq!(quiet.len(), 2, "no line without a refusal");
    }

    #[test]
    fn the_exports_doctor_names_the_tenant_export_policy_and_counts_bundles_served() {
        let lines = exports_doctor(Some(&Scrape("oml_exports_total 3\n".to_owned())));
        assert!(lines[0].starts_with("export_policy: own_root"), "{lines:?}");
        assert_eq!(
            lines[1],
            "3 bundle(s) served since start, each journalled as workspace.exported"
        );
        assert!(
            exports_doctor(None)[1].starts_with("bundles served: not read"),
            "named when the listener is not read"
        );
    }

    #[test]
    fn the_uploads_doctor_warns_that_the_object_store_is_unavailable_and_names_the_reason() {
        let down = Scrape(
            "oml_upload_sessions{state=\"open\"} 0\n\
             oml_upload_store_unavailable{reason=\"permission_denied\"} 1\n"
                .to_owned(),
        );
        let lines = uploads_doctor(Some(&down));
        assert_eq!(
            lines[1],
            "warn upload_store_unavailable: object store unavailable: permission_denied; the lead serves every other route, every upload answers upload.storage_failed until storage.url names a directory the lead can write, and the next sweep tries it again; the lead's log names the path under event upload_store_unavailable (docs/operations/upgrades.md)",
            "{lines:?}"
        );
        let up = Scrape("oml_upload_sessions{state=\"open\"} 0\n".to_owned());
        assert_eq!(
            uploads_doctor(Some(&up)).len(),
            1,
            "no warn while the store opens"
        );
    }

    #[test]
    fn the_uploads_doctor_counts_sessions_and_warns_on_unswept_objects_and_alarming_refusals() {
        assert_eq!(
            uploads_doctor(Some(&Scrape("oml_http_requests 1\n".to_owned()))),
            vec!["off (this server keeps no upload sessions)".to_owned()]
        );
        let quiet = Scrape(
            "oml_upload_sessions{state=\"open\"} 2\n\
             oml_upload_sessions{state=\"completed\"} 1\n\
             oml_upload_unbound_bytes 300\n\
             oml_upload_stored_bytes 512\n\
             oml_upload_unswept 0\n\
             oml_upload_sweep_failures_total 0\n\
             oml_upload_refusals_total{code=\"upload.incomplete\"} 3\n"
                .to_owned(),
        );
        assert_eq!(
            uploads_doctor(Some(&quiet)),
            vec![
                "2 open, 1 completed and unbound, 0 bound, 0 expired, 0 abandoned, 0 failed; 300 bytes declared unbound, 512 bytes stored".to_owned(),
                "upload_refusals since start: upload.incomplete 3".to_owned(),
            ]
        );
        let loud = Scrape(
            "oml_upload_sessions{state=\"expired\"} 1\n\
             oml_upload_unswept 1\n\
             oml_upload_sweep_failures_total 2\n\
             oml_upload_refusals_total{code=\"upload.digest_mismatch\"} 1\n"
                .to_owned(),
        );
        let lines = uploads_doctor(Some(&loud));
        assert!(lines[1].starts_with("warn upload_unswept: 1 "), "{lines:?}");
        assert!(
            lines[2].starts_with("warn upload_sweep_failed: 2 "),
            "{lines:?}"
        );
        assert!(
            lines[3].starts_with("warn upload_refusals since start: upload.digest_mismatch 1;"),
            "{lines:?}"
        );
        assert_eq!(
            uploads_doctor(None),
            vec!["not read: set OML_METRICS_URL to the lead's metrics listener".to_owned()]
        );
    }

    #[test]
    fn the_lifecycle_doctor_warns_of_stranded_held_successors_and_erased_versions_with_a_live_key()
    {
        let some = json!({
            "scanner_version": "oml-scan/1.2",
            "held_successors_stranded": 2,
            "erased_keys_live": 1,
        });
        assert_eq!(
            lifecycle_doctor(Some(&Ok(some))),
            vec![
                "warn memory.held_successor_stranded: 2 held version(s) replace a version that is no longer their claim's promoted tip, so no verdict can approve them; their authors withdraw them or a reviewer rejects them".to_owned(),
                "warn lake.erased_key_live: 1 erased version(s) still have a live data key, which the erase destroys in its own transaction; the lake writer destroys it when it drains the erase, so check that it runs".to_owned(),
            ]
        );
        let none = json!({ "held_successors_stranded": 0, "erased_keys_live": 0 });
        assert_eq!(
            lifecycle_doctor(Some(&Ok(none))),
            vec![
                "memory.held_successor_stranded: 0".to_owned(),
                "lake.erased_key_live: 0".to_owned()
            ]
        );
        assert!(lifecycle_doctor(Some(&Ok(json!({ "scanner_version": "x" })))).is_empty());
        assert_eq!(
            lifecycle_doctor(None),
            vec![
                "memory.held_successor_stranded: not read; an admin key reads it".to_owned(),
                "lake.erased_key_live: not read; an admin key reads it".to_owned(),
                "admin.investigation_open_long: not read; an admin key reads it".to_owned(),
                "admin.fix_unconfirmed: not read; an admin key reads it".to_owned(),
                "admin.fix_loosens_guardrail: not read; an admin key reads it".to_owned(),
                "admin.fix_without_person_reviewer: not read; an admin key reads it".to_owned(),
                "admin.fix_admin_person_only: not read; an admin key reads it".to_owned(),
                "admin.fix_reverted: not read; an admin key reads it".to_owned(),
                "key.link_pending: not read; an admin key reads it".to_owned(),
                "web.email_pending: not read; an admin key reads it".to_owned(),
                "admin.act_pending_confirmation: not read; an admin key reads it".to_owned(),
                "admin.erase_shred_pending: not read; an admin key reads it".to_owned(),
                "admin.act_sole_admin: not read; an admin key reads it".to_owned(),
                "admin.act_without_reason: not read; an admin key reads it".to_owned(),
                "admin.act_expired: not read; an admin key reads it".to_owned(),
                "admin.narrowing_burst: not read; an admin key reads it".to_owned(),
                "admin.offensive_narrowing: not read; an admin key reads it".to_owned(),
                "admin.key_suspended: not read; an admin key reads it".to_owned(),
                "admin.key_unlinked_suspended: not read; an admin key reads it".to_owned(),
                "admin.dispute_open: not read; an admin key reads it".to_owned()
            ]
        );
        assert_eq!(
            lifecycle_doctor(Some(&Ok(json!({ "erased_keys_live": "many" })))),
            vec![
                "warn lake.erased_key_live: the server's erased_keys_live is \"many\", not a count"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn the_lifecycle_doctor_warns_of_an_investigation_open_more_than_seven_days() {
        assert_eq!(
            lifecycle_doctor(Some(&Ok(json!({ "investigations_open_long": 1 })))),
            vec!["warn admin.investigation_open_long: 1 admin investigation(s) open for more than 7 days; the key that opened each closes it with oml admin investigate --close <id>".to_owned()]
        );
        assert_eq!(
            lifecycle_doctor(Some(&Ok(json!({ "investigations_open_long": 0 })))),
            vec!["admin.investigation_open_long: 0".to_owned()]
        );
    }

    #[test]
    fn the_lifecycle_doctor_warns_of_unconfirmed_admin_fixes_with_the_oldests_age() {
        let report = json!({
            "fixes_unconfirmed": 2,
            "fix_oldest_unconfirmed_seconds": 7200,
            "fixes_loosening_guardrail": 1,
            "fixes_without_person_reviewer": 0,
            "fixes_admin_person_only": 1,
            "fixes_reverted": 0,
        });
        assert_eq!(
            lifecycle_doctor(Some(&Ok(report))),
            vec![
                "warn admin.fix_unconfirmed: 2 admin fix(es) no person has confirmed or reverted; a person's reviewer key lists them with oml review fixes; the oldest is 7200 s old".to_owned(),
                "warn admin.fix_loosens_guardrail: 1 unconfirmed admin fix(es) took a served instruction out of service or rewrote one; a person confirms or reverts each with oml review fixes".to_owned(),
                "admin.fix_without_person_reviewer: 0".to_owned(),
                "warn admin.fix_admin_person_only: 1 unconfirmed admin fix(es) sit on a workspace where only the admin's own person holds reviewer; another person confirms them".to_owned(),
                "admin.fix_reverted: 0".to_owned(),
            ]
        );
    }

    #[test]
    fn the_lifecycle_doctor_warns_of_sign_in_emails_waiting_on_their_persons_acceptance() {
        assert_eq!(
            lifecycle_doctor(Some(&Ok(json!({ "web_emails_pending": 3 })))),
            vec!["warn web.email_pending: 3 sign-in email(s) wait on their person's acceptance; each signs in to nothing until that person accepts it on the console's Keys page, or the person or an admin removes it".to_owned()]
        );
        assert_eq!(
            lifecycle_doctor(Some(&Ok(json!({ "web_emails_pending": 0 })))),
            vec!["web.email_pending: 0".to_owned()]
        );
    }

    #[test]
    fn the_lifecycle_doctor_warns_of_erases_whose_shred_waits_with_the_oldest_age() {
        let report = json!({
            "admin_erase_shreds_pending": 1,
            "admin_erase_shred_oldest_seconds": 7200,
        });
        assert_eq!(
            lifecycle_doctor(Some(&Ok(report))),
            vec!["warn admin.erase_shred_pending: 1 erase(s) demoted their claim and keep its bytes until a second admin person confirms the shred with oml admin confirm <act>, or declines it; the oldest is 7200 s old".to_owned()]
        );
    }

    #[test]
    fn the_lifecycle_doctor_warns_of_admin_acts_waiting_on_a_second_admin_person() {
        let report = json!({
            "admin_acts_pending": 2,
            "admin_act_oldest_pending_seconds": 3600,
            "admin_acts_sole_admin": 0,
            "admin_acts_without_reason": 1,
            "admin_acts_expired": 0,
        });
        assert_eq!(
            lifecycle_doctor(Some(&Ok(report))),
            vec![
                "warn admin.act_pending_confirmation: 2 admin act(s) wait on a second admin person; each takes effect only once confirmed with oml admin confirm <act>, and expires as declined after 7 days; the oldest is 3600 s old".to_owned(),
                "admin.act_sole_admin: 0".to_owned(),
                "warn admin.act_without_reason: 1 admin act(s) carry no reason; send --reason with each admin act".to_owned(),
                "admin.act_expired: 0".to_owned(),
            ]
        );
    }

    #[test]
    fn the_lifecycle_doctor_warns_of_key_links_waiting_on_their_persons_acceptance() {
        assert_eq!(
            lifecycle_doctor(Some(&Ok(json!({ "key_links_pending": 2 })))),
            vec!["warn key.link_pending: 2 key link(s) wait on their person's acceptance; each key acts for no one, and an admin key among them takes no fix, until that person accepts the link on the console's Keys page".to_owned()]
        );
        assert_eq!(
            lifecycle_doctor(Some(&Ok(json!({ "key_links_pending": 0 })))),
            vec!["key.link_pending: 0".to_owned()]
        );
    }

    #[test]
    fn the_withdrawn_queue_doctor_fails_on_a_withdrawn_version_left_in_a_review_queue() {
        let some = json!({ "scanner_version": "oml-scan/1.2", "withdrawn_in_queue": 2 });
        assert_eq!(
            withdrawn_queue_doctor(Some(&Ok(some))),
            vec!["fail memory.withdrawn_in_queue: 2 withdrawn version(s) are still in a review queue, which a withdrawal's own transaction rules out; reviewers may be served them; report it with the server's logs".to_owned()]
        );
        let none = json!({ "scanner_version": "oml-scan/1.2", "withdrawn_in_queue": 0 });
        assert_eq!(
            withdrawn_queue_doctor(Some(&Ok(none))),
            vec!["memory.withdrawn_in_queue: 0".to_owned()]
        );
        assert!(
            withdrawn_queue_doctor(Some(&Ok(json!({ "scanner_version": "oml-scan/1.2" }))))
                .is_empty()
        );
        assert_eq!(
            withdrawn_queue_doctor(None),
            vec!["memory.withdrawn_in_queue: not read; an admin key reads it".to_owned()]
        );
        assert_eq!(
            withdrawn_queue_doctor(Some(&Err("503 unavailable".to_owned()))),
            vec!["warn memory.withdrawn_in_queue: not read (503 unavailable)".to_owned()]
        );
        let odd = json!({ "scanner_version": "oml-scan/1.2", "withdrawn_in_queue": "many" });
        assert_eq!(
            withdrawn_queue_doctor(Some(&Ok(odd))),
            vec!["warn memory.withdrawn_in_queue: the server's withdrawn_in_queue is \"many\", not a count".to_owned()]
        );
    }

    #[test]
    fn the_approval_doctor_names_instructions_approved_by_a_key_that_is_not_a_persons() {
        let report = json!({
            "promoted_unapproved": 0, "instructions_without_person_approval": 2,
        });
        assert_eq!(
            approval_doctor(Some(&Ok(report))),
            vec![
                "approval_unmatched: 0".to_owned(),
                "warn instruction_approvals: 2 instruction (steering, guardrail or skill) version(s) in force were approved by a key that is not a person's; a person can review each again by superseding it (ADR-0080 d16)".to_owned(),
            ]
        );
        // Nothing at zero, nothing from a server before the count, and an
        // odd value is named, never read as zero.
        let zero = json!({ "promoted_unapproved": 0, "instructions_without_person_approval": 0 });
        assert_eq!(
            approval_doctor(Some(&Ok(zero))),
            vec!["approval_unmatched: 0".to_owned()]
        );
        let odd = json!({ "promoted_unapproved": 0, "instructions_without_person_approval": "x" });
        assert_eq!(
            approval_doctor(Some(&Ok(odd))),
            vec![
                "approval_unmatched: 0".to_owned(),
                "warn instruction_approvals: the server's instructions_without_person_approval is \"x\", not a count".to_owned(),
            ]
        );
    }

    #[test]
    fn the_imports_doctor_warns_only_on_imports_running_past_the_hour_and_on_unprovenanced_versions()
     {
        // A run under an hour is running, not stopped (ADR-0084 I11).
        let live = Scrape(
            "oml_imports{state=\"running\"} 1\n\
             oml_imports_stopped 0\n\
             oml_imports_unprovenanced 0\n"
                .to_owned(),
        );
        assert_eq!(
            imports_doctor(Some(&live)),
            vec!["0 done, 0 failed, 1 running, 0 stopped".to_owned()]
        );
        let stopped = Scrape(
            "oml_imports{state=\"running\"} 2\n\
             oml_imports_stopped 1\n\
             oml_imports_unprovenanced 3\n"
                .to_owned(),
        );
        let lines = imports_doctor(Some(&stopped));
        assert!(
            lines[1].starts_with("warn imports_stopped: 1 "),
            "{lines:?}"
        );
        assert!(
            lines[2].starts_with("warn imports_unprovenanced: 3 "),
            "{lines:?}"
        );
    }

    #[test]
    fn the_uploads_doctor_warns_on_staging_keys_still_set_a_day_after_binding() {
        let stale = Scrape(
            "oml_upload_sessions{state=\"bound\"} 1\n\
             oml_upload_staging_stale 1\n"
                .to_owned(),
        );
        let lines = uploads_doctor(Some(&stale));
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("warn upload_staging_stale: 1 ")),
            "{lines:?}"
        );
    }

    #[test]
    fn the_imports_doctor_counts_imports_and_warns_on_stopped_ones() {
        assert_eq!(
            imports_doctor(Some(&Scrape("oml_http_requests 1\n".to_owned()))),
            vec!["off (this server runs no imports)".to_owned()]
        );
        let quiet = Scrape(
            "oml_imports{state=\"done\"} 3\n\
             oml_imports{state=\"running\"} 0\n\
             oml_imports_allow_older 0\n"
                .to_owned(),
        );
        assert_eq!(
            imports_doctor(Some(&quiet)),
            vec!["3 done, 0 failed, 0 running, 0 stopped".to_owned()]
        );
        let loud = Scrape(
            "oml_imports{state=\"running\"} 1\n\
             oml_imports_stopped 1\n\
             oml_imports_allow_older 2\n\
             oml_import_refusals_total{code=\"bundle.digest_mismatch\"} 1\n"
                .to_owned(),
        );
        let lines = imports_doctor(Some(&loud));
        assert!(
            lines[1].starts_with("warn imports_stopped: 1 "),
            "{lines:?}"
        );
        assert!(lines[2].starts_with("allow_older: 2 "), "{lines:?}");
        assert_eq!(
            lines[3], "import_refusals since start: bundle.digest_mismatch 1",
            "{lines:?}"
        );
        assert_eq!(
            imports_doctor(None),
            vec!["not read: set OML_METRICS_URL to the lead's metrics listener".to_owned()]
        );
    }

    #[test]
    fn a_crews_workspace_with_held_steering_and_no_person_reviewer_is_named_by_doctor() {
        let queue = json!({ "items": [
            { "path": "universe.crew", "held": 4, "held_instructions": 2, "person_reviewers": 0 },
            { "path": "universe.team", "held": 3, "held_instructions": 1, "person_reviewers": 2 },
            { "path": "universe.notes", "held": 5, "held_instructions": 0, "person_reviewers": 0 },
            // A server before the counts names neither, and says nothing.
            { "path": "universe.old", "held": 1 },
        ]});
        assert_eq!(
            instructions_unreviewable_doctor(Some(&Ok(queue))),
            vec!["warn instructions_unreviewable: universe.crew holds 2 instruction (steering, guardrail or skill) version(s) and no person's reviewer key reads it; issue a reviewer key with --used-by person and a reviewer grant there, or approve in the console (review.person_required, instruction_needs_person)".to_owned()]
        );
        // Every workspace has a person who can approve: no line.
        let clean = json!({ "items": [
            { "path": "universe.team", "held": 3, "held_instructions": 1, "person_reviewers": 1 },
        ]});
        assert!(instructions_unreviewable_doctor(Some(&Ok(clean))).is_empty());
        assert_eq!(
            instructions_unreviewable_doctor(None),
            vec!["instructions_unreviewable: not read; an admin key reads it".to_owned()]
        );
        assert_eq!(
            instructions_unreviewable_doctor(Some(&Err("503 unavailable".to_owned()))),
            vec!["warn instructions_unreviewable: not read (503 unavailable)".to_owned()]
        );
        let odd = json!({ "items": [
            { "path": "universe.crew", "held": 1, "held_instructions": "many", "person_reviewers": 0 },
        ]});
        assert_eq!(
            instructions_unreviewable_doctor(Some(&Ok(odd))),
            vec!["warn instructions_unreviewable: the server's counts for universe.crew are \"many\" and 0, not counts".to_owned()]
        );
    }

    #[test]
    fn the_approval_doctor_warns_when_an_admins_read_fails_and_when_the_count_is_not_a_count() {
        // The admin's read failed: a warn naming why, not "an admin key
        // reads it", which this key is.
        assert_eq!(
            approval_doctor(Some(&Err("503 unavailable".to_owned()))),
            vec!["warn approval_unmatched: not read (503 unavailable)".to_owned()]
        );
        // The member is there and is no unsigned integer.
        for odd in [json!("many"), json!(-1), json!(1.5), Value::Null] {
            let report = json!({ "scanner_version": "oml-scan/1.2", "promoted_unapproved": odd });
            assert_eq!(
                approval_doctor(Some(&Ok(report))),
                vec![format!(
                    "warn approval_unmatched: the server's promoted_unapproved is {odd}, not a count"
                )],
            );
        }
    }

    #[test]
    fn an_anchor_whose_rev_has_a_multibyte_character_across_byte_12_is_one_line_and_no_panic() {
        // The server's rev is text: `é` is two bytes, at bytes 11 and 12.
        let odd = "0123456789aébcdef";
        assert!(!odd.is_char_boundary(12));
        assert_eq!(
            anchor_line(&json!({"repo": "r", "rev": odd, "path": "docs/a.md", "fragment": "x"})),
            "r @ 0123456789aé docs/a.md#x"
        );
        assert_eq!(
            anchor_line(&json!({"repo": "r", "rev": "0123456789abcdef0123", "path": "docs/a.md"})),
            "r @ 0123456789ab docs/a.md"
        );
        assert_eq!(anchor_line(&Value::Null), "none");
    }

    #[test]
    fn the_web_doctor_reads_failed_exchanges_from_the_metrics_listener() {
        let ready = (200, json!({"web": {"providers": ["google", "github"]}}));
        let scrape = Scrape(
            "oml_web_exchanges_failed_total{provider=\"google\",reason=\"busy\"} 1\n\
             oml_web_exchanges_failed_total{provider=\"google\",reason=\"error_answer\"} 1\n"
                .to_owned(),
        );
        let lines = web_doctor(Some(&ready), None, Some(&scrape));
        assert_eq!(lines[0], "on (google, github)");
        assert_eq!(
            lines[1],
            "warn web_provider_google: 2 failed exchanges since start (1 busy, 1 error_answer)"
        );
        assert_eq!(lines.len(), 2, "{lines:?}");
    }

    #[test]
    fn the_web_doctor_warns_when_the_lead_has_no_web_client_identity() {
        let bare = (
            200,
            json!({"web": {"providers": ["google"]}, "web_client": "none"}),
        );
        assert_eq!(
            web_doctor(Some(&bare), None, None),
            vec![
                "on (google)".to_owned(),
                "warn web: no web client identity set; oml-web cannot reach the API (set web.client_secret on the lead and on oml-web)".to_owned(),
            ]
        );
        let set = (
            200,
            json!({"web": {"providers": ["google"]}, "web_client": "secret"}),
        );
        assert_eq!(
            web_doctor(Some(&set), None, None),
            vec!["on (google)".to_owned()]
        );
    }

    #[test]
    fn doctor_names_journal_unmapped_former_name_when_the_lead_reports_one() {
        let unmapped = (503, json!({"checks": {"former_names": "unmapped"}}));
        let line = former_names_doctor(Some(&unmapped)).expect("a line");
        assert!(
            line.starts_with("fail journal_unmapped_former_name:"),
            "{line}"
        );
        let ok = (200, json!({"checks": {"former_names": "ok"}}));
        assert!(
            former_names_doctor(Some(&ok))
                .expect("a line")
                .starts_with("ok journal_unmapped_former_name:")
        );
        let older = (200, json!({"checks": {"postgres": "ok"}}));
        assert_eq!(former_names_doctor(Some(&older)), None);
    }

    #[test]
    fn doctor_names_people_near_a_quota_pending_deletions_and_a_shared_address_limit() {
        let overview = serde_json::json!({
            "mode": "open",
            "address_limit_shared": true,
            "users": [
                {"user": "ua", "under_quotas": true, "disabled": false,
                 "usage": {"memories": 850, "keys": 1, "connections": 0},
                 "quotas": {"memories": 1000, "keys": 5, "connections": 5}},
                {"user": "ub", "under_quotas": true, "disabled": false,
                 "usage": {"memories": 10, "keys": 5, "connections": 0},
                 "quotas": {"memories": 1000, "keys": 5, "connections": 5}},
                {"user": "op", "under_quotas": false, "disabled": false,
                 "usage": {"memories": 5000, "keys": 9, "connections": 0},
                 "quotas": {"memories": 1000, "keys": 5, "connections": 5}}
            ],
            "deletions": [{"user": "ub", "requested_at": "2026-11-06T10:00:00.000000Z"}]
        });
        let lines = signup_doctor(Some(&overview));
        assert_eq!(
            lines,
            vec![
                "warn signup: open with no server.trusted_proxies; every sign-up shares one address limit",
                "warn quota: 2 users at or above 80% of a quota (keys: 1, memories: 1)",
                "warn signup: 1 deletion requests pending, oldest requested 2026-11-06T10:00:00.000000Z; close each on /web/admin/users",
            ]
        );
        let quiet = serde_json::json!({"mode": "allowlist", "address_limit_shared": true, "users": [], "deletions": []});
        assert!(signup_doctor(Some(&quiet)).is_empty());
        assert!(signup_doctor(None).is_empty());
    }

    #[test]
    fn doctor_warns_when_a_tenant_writing_keys_default_workspace_lies_in_a_users_root() {
        let node = serde_json::json!({ "path": "universe.alice.project" });
        let tenant_writer = serde_json::json!({ "role": "contributor", "user": null });
        let tenant_submitter = serde_json::json!({ "role": "submitter", "user": null });
        assert_eq!(
            user_root_writes_doctor(Some(&tenant_submitter), Some(&node), Some("alice")),
            user_root_writes_doctor(Some(&tenant_writer), Some(&node), Some("alice")),
        );
        assert_eq!(
            user_root_writes_doctor(Some(&tenant_writer), Some(&node), Some("alice")).as_deref(),
            Some(
                "warn: the default workspace universe.alice.project lies in the root of user alice, where only alice's keys start memories; this tenant key's new memories there are refused (submission.user_root_refused); issue the agent a key with --user alice"
            )
        );
        // A shared default, a user key in its own root and a key that
        // writes nothing are told nothing.
        assert_eq!(
            user_root_writes_doctor(Some(&tenant_writer), Some(&node), None),
            None
        );
        let alices = serde_json::json!({ "role": "contributor", "user": { "name": "alice" } });
        assert_eq!(
            user_root_writes_doctor(Some(&alices), Some(&node), Some("alice")),
            None
        );
        let reviewer = serde_json::json!({ "role": "reviewer", "user": null });
        assert_eq!(
            user_root_writes_doctor(Some(&reviewer), Some(&node), Some("alice")),
            None
        );
    }

    #[test]
    fn doctor_warns_for_an_admin_key_whose_row_names_a_persons_root() {
        let overview = serde_json::json!({
            "mode": "allowlist",
            "address_limit_shared": false,
            "users": [
                {"user": "alice", "under_quotas": false, "disabled": false,
                 "admin_keys": ["stray-admin"],
                 "usage": {}, "quotas": {}},
                {"user": "bob", "under_quotas": false, "disabled": false,
                 "admin_keys": [], "usage": {}, "quotas": {}},
                {"user": "carol", "under_quotas": false, "disabled": false,
                 "usage": {}, "quotas": {}}
            ],
            "deletions": []
        });
        assert_eq!(
            signup_doctor(Some(&overview)),
            vec![
                "warn keys: admin key stray-admin names the user root of alice, which no verb does; disabling alice is refused until the row's user_root_id is cleared"
            ]
        );
    }

    #[test]
    fn doctor_names_drifted_counters_and_accounts_on_an_undeclared_profile_and_nothing_when_clean()
    {
        let report = serde_json::json!({
            "drift": [
                {"account": "a1", "user": "ua", "meter": "memories", "counter": 12, "rows": 11},
                {"account": "a1", "user": "ua", "meter": "text_bytes", "counter": 40, "rows": 41}
            ],
            "undeclared_profiles": [
                {"account": "a2", "user": "ub", "profile": "pro"},
                {"account": "a3", "user": "uc", "profile": "pro"}
            ]
        });
        assert_eq!(
            usage_doctor(Some(&Ok(report))),
            vec![
                "warn usage: 1 accounts drift from their rows (ua memories counter 12 rows 11, ua text_bytes counter 40 rows 41)",
                "warn usage: 2 accounts on profile pro, which the configuration does not declare; their writes are refused limit.quota until an admin reassigns them or declares it",
            ]
        );
        let clean = serde_json::json!({"drift": [], "undeclared_profiles": []});
        assert!(usage_doctor(Some(&Ok(clean))).is_empty());
        assert!(usage_doctor(None).is_empty());
    }

    /// The server's refusal of a key as doctor's whoami reads it.
    fn refused(status: u16, code: &str, details: Value) -> CliError {
        CliError::Refused {
            status,
            code: code.to_owned(),
            message: String::new(),
            rule: String::new(),
            envelope: serde_json::json!({ "error": { "code": code, "details": details } }),
        }
    }

    #[test]
    fn doctor_fails_an_expired_key_with_its_date_and_warns_of_a_key_past_its_rate() {
        let expired = refused(
            401,
            "auth.invalid",
            serde_json::json!({ "reason": "expired", "expiry": "2026-12-01T17:00:00.000000Z" }),
        );
        assert_eq!(
            key_doctor(Some(&Err(expired))),
            vec![
                "fail expired: this key ended at 2026-12-01T17:00:00.000000Z; an admin moves its date with `oml keys limit <key> --expires <instant>`, or issues another key"
            ]
        );
        let limited = refused(
            429,
            "limit.rate",
            serde_json::json!({ "quota": "requests_per_minute", "limit": 60 }),
        );
        assert_eq!(
            key_doctor(Some(&Err(limited))),
            vec![
                "warn rate: this key reached its 60 requests per minute; the next minute admits it again"
            ]
        );
        // A key the server does not know is the whoami line's alone.
        let unknown = refused(
            401,
            "auth.invalid",
            serde_json::json!({ "reason": "unknown" }),
        );
        assert!(key_doctor(Some(&Err(unknown))).is_empty());
    }

    #[test]
    fn doctor_names_the_instant_a_key_ends_and_its_rate_and_nothing_for_a_key_with_neither() {
        let limited = serde_json::json!({
            "expiry": "2026-12-01T17:00:00.000000Z",
            "requests_per_minute": 60,
        });
        assert_eq!(
            key_doctor(Some(&Ok(limited))),
            vec![
                "ok expiry: this key ends at 2026-12-01T17:00:00.000000Z; an admin moves its date with `oml keys limit <key> --expires <instant>`",
                "ok rate: this key is held to 60 requests per minute, counted by each server process alone",
            ]
        );
        let plain = serde_json::json!({ "expiry": null, "requests_per_minute": null });
        assert!(key_doctor(Some(&Ok(plain))).is_empty());
        assert!(key_doctor(None).is_empty());
    }

    /// While a rotation's grace is open the doctor says until when the
    /// previous value works, as information (ADR-0096 d13).
    #[test]
    fn doctor_says_when_a_rotated_keys_previous_value_stops_while_its_grace_is_open() {
        let rotated = serde_json::json!({
            "name": "ada",
            "rotation_grace_until": "2026-11-26T10:00:00.000000Z",
        });
        assert_eq!(
            key_doctor(Some(&Ok(rotated))),
            vec![
                "info key.rotation_grace_open: this key's previous value still authenticates until 2026-11-26T10:00:00.000000Z; move every client to the new value before then",
            ]
        );
    }

    /// The doctor names the key as the server serves it, by `whoami`'s
    /// `without_recall`: a migrated v0.8.3 write-only user key holds a
    /// reader grant, the `universe` node grant, and is named all the same,
    /// since that grant reaches none of the nodes it writes (ADR-0093 d3, as
    /// read on 2026-11-20).
    #[test]
    fn doctor_names_a_migrated_write_only_user_key_as_key_contributor_without_recall() {
        let migrated = serde_json::json!({
            "name": "ada-ingest",
            "role": "contributor",
            "grants": [
                { "role": "submitter", "workspace_node_id": "root", "covers": "subtree" },
                { "role": "reader", "workspace_node_id": "universe", "covers": "node" },
            ],
            "without_recall": true,
        });
        assert_eq!(
            key_doctor(Some(&Ok(migrated))),
            vec![
                "warn key_contributor_without_recall: ada-ingest is a contributor key that writes where no reader grant of it reaches, a writer key issued with --write-only before v0.8.4; it works as before, but issue a submitter key with `oml keys issue-agent <name> --role submitter` and cut this one off",
            ]
        );
        // A contributor that recalls where it writes, and a submitter, are
        // told nothing.
        let recalls = serde_json::json!({
            "name": "ada",
            "role": "contributor",
            "grants": [{ "role": "submitter" }, { "role": "reader" }],
            "without_recall": false,
        });
        assert!(key_doctor(Some(&Ok(recalls))).is_empty());
        let submitter = serde_json::json!({
            "name": "ada-ingest-2",
            "role": "submitter",
            "grants": [{ "role": "submitter" }],
            "without_recall": false,
        });
        assert!(key_doctor(Some(&Ok(submitter))).is_empty());
    }

    #[test]
    fn keys_limit_sends_only_the_limits_it_was_told_to_set_or_clear_and_refuses_none() {
        assert_eq!(
            key_limits_body(Some("2026-12-01T17:00:00Z"), false, None, false),
            Ok(serde_json::json!({ "expiry": "2026-12-01T17:00:00Z" }))
        );
        assert_eq!(
            key_limits_body(None, true, Some(60), false),
            Ok(serde_json::json!({ "expiry": null, "requests_per_minute": 60 }))
        );
        assert_eq!(
            key_limits_body(None, false, None, true),
            Ok(serde_json::json!({ "requests_per_minute": null }))
        );
        let refused = key_limits_body(None, false, None, false).expect_err("no limit named");
        assert!(refused.contains("nothing was sent"), "{refused}");
    }

    #[test]
    fn a_key_read_prints_its_expiry_marked_once_it_has_passed_and_its_rate() {
        let live = serde_json::json!({
            "expiry": "2026-12-01T17:00:00.000000Z", "expired": false, "requests_per_minute": 60,
        });
        assert_eq!(
            key_limit_lines(&live),
            vec![
                "expiry   : 2026-12-01T17:00:00.000000Z",
                "rate     : 60 requests per minute",
            ]
        );
        let ended = serde_json::json!({
            "expiry": "2026-12-01T17:00:00.000000Z", "expired": true, "requests_per_minute": null,
        });
        assert_eq!(
            key_limit_lines(&ended),
            vec!["expiry   : 2026-12-01T17:00:00.000000Z (expired: the key authenticates nothing)"]
        );
        let plain = serde_json::json!({ "expiry": null, "requests_per_minute": null });
        assert!(key_limit_lines(&plain).is_empty());
    }

    #[test]
    fn doctor_says_so_when_the_drift_report_could_not_be_read() {
        assert_eq!(
            usage_doctor(Some(&Err("503 store.unavailable".to_owned()))),
            vec![
                "warn usage: the drift report was not read (503 store.unavailable); the counters are unchecked against their rows"
            ]
        );
    }

    #[test]
    fn doctor_warns_when_a_time_pipeline_has_no_memory_that_declares_valid_time() {
        let none = serde_json::json!({ "valid_time": { "declared": 0, "expired": 0 } });
        let lines = valid_time_doctor(&["bm25-postings", "bm25-postings-time"], Some(&none));
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with("warn time: bm25-postings-time is enabled"),
            "{lines:?}"
        );
        let some = serde_json::json!({ "valid_time": { "declared": 12, "expired": 3 } });
        assert_eq!(
            valid_time_doctor(&["bm25-postings-time"], Some(&some)),
            vec!["time: 12 memories declare valid time; 3 ended and hidden from recall unless include_expired".to_owned()]
        );
        assert!(valid_time_doctor(&["bm25-postings"], Some(&none)).is_empty());
        let untimed =
            serde_json::json!({ "valid_time": { "declared": 0, "expired": 0, "untimed": 2 } });
        let lines = valid_time_doctor(&["bm25-postings"], Some(&untimed));
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].starts_with("warn as_of: 2 promoted versions"),
            "{lines:?}"
        );
        assert_eq!(
            valid_time_doctor(&["bm25-postings"], Some(&some)).len(),
            1,
            "the expired count shows under the default pipeline too"
        );
    }

    #[test]
    fn a_review_line_shows_the_valid_time_and_the_end_approving_gives() {
        let successor = serde_json::json!({
            "valid_from": "2026-06-01",
            "valid_until": null,
            "replaces": { "artifact_id": "x", "version": 1 },
            "replaces_valid_until": "2026-06-01T00:00:00.000000Z",
        });
        assert_eq!(
            valid_line(&successor),
            "from 2026-06-01, open; approving ends version 1 at 2026-06-01T00:00:00.000000Z"
        );
        let open = serde_json::json!({ "valid_from": null, "valid_until": null });
        assert_eq!(valid_line(&open), "none declared");
        let ended = serde_json::json!({ "valid_from": null, "valid_until": "2015" });
        assert_eq!(valid_line(&ended), "from any time, until 2015");
    }

    #[test]
    fn the_federation_doctor_names_a_widen_signal_that_failed_was_evicted_or_was_never_built() {
        let settings = |built: u64, unserved: u64, failed: u64| {
            json!({
                "enabled": true,
                "max_sources": 8,
                "wide_keys": [],
                "widen_signal": {
                    "built": built,
                    "unserved": unserved,
                    "evicted": if built == 7 { 2 } else { 0 },
                    "failed": failed,
                    "last_failure": if failed > 0 { Value::from("backend: timeout") } else { Value::Null },
                },
            })
        };
        assert!(federation_doctor(Some(&settings(3, 2, 0))).is_empty());
        assert_eq!(
            federation_doctor(Some(&settings(7, 0, 0))),
            vec![
                "warn 2 widen signals were dropped to keep this tenant or the lead within its share, each not rebuilt for 300 s: their scopes carry no hint meanwhile"
                    .to_owned()
            ]
        );
        assert!(federation_doctor(Some(&settings(0, 0, 0))).is_empty());
        assert_eq!(
            federation_doctor(Some(&settings(1, 0, 2))),
            vec![
                "warn the widen hint's signal failed to build 2 times since the lead started, last: backend: timeout; a scope recall without its signal carries no hint"
                    .to_owned()
            ]
        );
        assert_eq!(
            federation_doctor(Some(&settings(0, 5, 0))),
            vec![
                "warn 5 scope recalls found no widen signal and none was built since the lead started: no scope answer carries the widen hint until one is"
                    .to_owned()
            ]
        );
    }
}
