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
    KeyFile, configure_line, connect_code_printed, minted_code_printed, parse_ttl, setup_line,
    sh_quote, with_reader_beside_writers,
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
    /// Manage workareas.
    Workareas {
        #[command(subcommand)]
        command: WorkareasCommand,
    },
    /// Manage claim links and workarea brief entries, proposed and ratified by different keys.
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
    /// Tail journal event digests within the key's reader access.
    Events(EventsArgs),
    /// Sync a workarea with committed repository documents: extract, diff, write,
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
        /// New workarea name; it must not exist.
        #[arg(value_name = "WORKAREA")]
        workarea: String,
        /// Parent workarea.
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
}

#[derive(Subcommand)]
enum SetupCommand {
    /// Install the matching oml plugin through the claude CLI, passing the URL and key
    /// on stdin. Admin and operator keys are refused. Print commands with --print or
    /// when claude is unavailable. Without --url, use <https://memory.doublegate.io>.
    ClaudeCode(SetupClaudeCodeArgs),
}

#[derive(Args)]
struct SetupClaudeCodeArgs {
    /// Key file from `oml keys issue-agent --out`, used instead of --api-key and
    /// paired with --url or the default server.
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
        /// Scope workarea by id or name; omitted, the key's default.
        #[arg(long)]
        workarea: Option<String>,
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
        /// Scope workarea by id or name; omitted, the key's default.
        #[arg(long)]
        workarea: Option<String>,
    },
}

#[derive(Subcommand)]
enum ChatCommand {
    /// Open your `chat` workarea under your user root and print OML_CHAT_WORKAREA
    /// for plugin capture and recall. Requires a person's key.
    Init {
        /// Parent by id or name; omitted, your key's user root.
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
        /// workarea.
        #[arg(long, value_name = "DURATION")]
        ttl: Option<String>,
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
    /// reader | writer | dreamer | admin
    #[arg(long)]
    role: String,
    /// agent | human | service
    #[arg(long, default_value = "agent")]
    held_by: String,
    /// Default workarea (id or name) for provider requests without one; must be
    /// covered by a grant. Defaults to the first grant's workarea.
    #[arg(long)]
    default_workarea: Option<String>,
    /// Grant <workarea>:<role>, or <workarea>:<role>:node for one node; workarea
    /// may be id or name. Repeatable. Writer grants include reader access unless
    /// --write-only.
    #[arg(long = "grant", value_parser = parse_grant)]
    grants: Vec<(String, String)>,
    /// Crew record (id or name), fixed for the key's life.
    #[arg(long)]
    crew: Option<String>,
    /// User the key acts for. Grants must be under `universe.<user>`, where
    /// workarea names resolve; the first key creates that root.
    #[arg(long)]
    user: Option<String>,
    /// Send writer grants without matching reader grants.
    #[arg(long)]
    write_only: bool,
    /// Principal the key acts as.
    #[arg(long)]
    principal: Option<String>,
    #[arg(long)]
    label: Option<String>,
    /// Guardrail scope: workarea loads only the default workarea's guardrails;
    /// ancestors also loads every readable ancestor. Defaults to ancestors.
    #[arg(long, value_parser = ["workarea", "ancestors"])]
    guardrail_scope: Option<String>,
    /// Key profile: curator is an agent-held dreamer that narrows within an
    /// in-progress workarea and never promotes.
    #[arg(long, value_parser = ["curator"])]
    profile: Option<String>,
}

#[derive(Args)]
struct IssueAgentArgs {
    /// Key name, unique in the tenant and never released; not derived from the principal.
    key_name: String,
    /// Principal the key acts as; give each agent its own.
    #[arg(long)]
    principal: String,
    /// Workarea (id or name) to write and recall, and its default. With --user,
    /// optional and resolved in that user's namespace; defaults to the user's root.
    #[arg(long)]
    workarea: Option<String>,
    /// User the key acts for, under `universe.<user>`; the first key for a user
    /// creates that root.
    #[arg(long)]
    user: Option<String>,
    /// Crew record (id or name), fixed for the key's life.
    #[arg(long)]
    crew: Option<String>,
    /// Who holds the key.
    #[arg(long, default_value = "agent", value_parser = ["agent", "human"])]
    held_by: String,
    /// Send the writer grant without a matching reader grant.
    #[arg(long)]
    write_only: bool,
    /// Write the key to a new 0600 file without overwriting and print a line that
    /// reads it instead of the value.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Issue a claude.ai custom-connector key with a single-use connect code
    /// instead of a key value.
    #[arg(long)]
    connect_code: bool,
    /// Guardrail scope: workarea loads only the default workarea's guardrails;
    /// ancestors also loads every readable ancestor. Defaults to ancestors.
    #[arg(long, value_parser = ["workarea", "ancestors"])]
    guardrail_scope: Option<String>,
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
        /// Workarea (id or name) whose subtree the key can no longer access.
        subtree: String,
        /// Reason for denial; accepted by the server but not journalled.
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand)]
enum WorkareasCommand {
    /// Create a child workarea in development status.
    Create {
        /// Parent workarea, by id or name.
        parent: String,
        /// New workarea name, unique in the tenant and immutable.
        name: String,
        /// Crew record to assign, by id or name.
        #[arg(long)]
        crew: Option<String>,
    },
    /// List a workarea tree with each node's path, id, status, and crew.
    Tree {
        /// Root workarea, by id or name.
        #[arg(value_name = "ROOT", conflicts_with = "root")]
        positional_root: Option<String>,
        /// Root workarea, by id or name.
        #[arg(long, conflicts_with = "positional_root")]
        root: Option<String>,
    },
    /// Show a workarea's path, status, crew, and children.
    Show {
        /// Workarea id or name.
        workarea: String,
    },
    /// Set in_progress to recall labelled unreviewed memories within the workarea,
    /// or standard to commit. Person-held dreamer keys only; commit is refused
    /// while items are held.
    Mode {
        /// Workarea id or name.
        workarea: String,
        /// `in_progress`, or `standard` to commit.
        #[arg(value_parser = ["in_progress", "standard"])]
        mode: String,
    },
    /// Set whether recalls scoped elsewhere may search this workarea when
    /// they ask for wider sources; grants nothing. Person-held dreamer keys or
    /// an admin key; journals workarea.federation_set.
    Federation {
        /// Workarea id or name.
        workarea: String,
        /// `on` lets federated recalls search it (the default); `off` declines.
        #[arg(value_parser = ["on", "off"])]
        accept: String,
    },
    /// Set a workarea's crew record (admin key).
    Crew {
        #[command(subcommand)]
        command: WorkareaCrewCommand,
    },
    /// Move through development, committed, and validated; each step needs
    /// proposal and ratification by different keys.
    Status {
        #[command(subcommand)]
        command: WorkareaStatusCommand,
    },
}

#[derive(Subcommand)]
enum WorkareaStatusCommand {
    /// Propose the next forward rung; it waits for a second identity.
    Propose {
        /// The workarea, by id or name.
        workarea: String,
        /// The rung proposed: committed or validated.
        #[arg(long)]
        to: String,
    },
    /// Ratify a pending proposal under a key that did not propose it.
    Ratify {
        /// The workarea, by id or name.
        workarea: String,
        /// The proposal's transition id.
        transition_id: String,
    },
    /// The workarea's live proposals and the review policy in force.
    Pending {
        /// The workarea, by id or name.
        workarea: String,
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
        /// For claim, filter by subtree; for brief, required workarea.
        #[arg(long)]
        workarea: Option<String>,
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
    /// Propose a promoted claim for a workarea's brief.
    Brief {
        /// Claim artifact id.
        artifact: String,
        /// Workarea id or name.
        workarea: String,
    },
}

#[derive(Subcommand)]
enum WorkareaCrewCommand {
    /// Set the workarea's crew record, including on validated workareas;
    /// journals workarea.crew_updated.
    Set {
        /// Workarea id or name.
        workarea: String,
        /// Crew record id or name.
        #[arg(long)]
        crew: String,
    },
}

#[derive(Subcommand)]
enum AdminCommand {
    /// Read one memory with text and attribution, or search all organisation
    /// memories with --query, --workarea, or --state; journals memory.admin_read.
    Read {
        /// Memory artifact id; omit to search.
        memory: Option<String>,
        /// Full-text search using recall matching.
        #[arg(long)]
        query: Option<String>,
        /// Limit to this workarea subtree (id, name, or qualified name).
        #[arg(long)]
        workarea: Option<String>,
        /// held, promoted, rejected, superseded or demoted.
        #[arg(long)]
        state: Option<String>,
    },
    /// Report by rule and location which stored versions current rules would
    /// refuse or hold; never prints text or changes data.
    Rescan {
        /// Also record each held version's findings under current rules, so
        /// a held version written before an upgrade is served inside an
        /// in-progress workarea again when nothing is found; journals
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
        /// The most workareas a wide recall searches beside its scope, and
        /// the most a sources list names, 0 to 64.
        #[arg(long)]
        max_sources: Option<u32>,
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
    },
    /// Remove an email, end its sessions, and journal web.email_removed; the
    /// user's agent keys keep working.
    Remove {
        /// Email address.
        email: String,
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
    /// Workarea (id or name) to sync; with --pr, the parent of `<workarea>-pr-<n>`.
    #[arg(long)]
    workarea: String,
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
    /// Propose source-stated edges between promoted tips (dreamer key).
    #[arg(long, conflicts_with = "check")]
    edges: bool,
    /// With --edges, ratify matching pending edges proposed by another key.
    #[arg(long, requires = "edges")]
    ratify: bool,
    /// Allow the maintainer's public server and record consent for this clone.
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
    /// into `<workarea>-pr-<n>`.
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
    /// guardrail; defaults to memory, or the target's kind with --replaces.
    #[arg(long)]
    kind: Option<String>,
    /// Guardrail severity: never, must, or should; required for non-retirement guardrails.
    #[arg(long, value_parser = ["never", "must", "should"])]
    severity: Option<String>,
    /// Guardrail oml-gr/1 rule as one JSON object; deny or ask, never allow.
    #[arg(long, value_parser = parse_rule)]
    rule: Option<serde_json::Value>,
    /// Lineage artifact id; repeatable. Journals submissions with lineage as learn.
    #[arg(long = "derives-from")]
    derives_from: Vec<String>,
    /// Workarea (id or name); defaults to the key's default, or the target's
    /// with --replaces, which it may not change.
    #[arg(long)]
    workarea: Option<String>,
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
    /// Recall only memory or derived_fact kinds; repeatable.
    #[arg(long, requires = "query")]
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
    /// Where to search: scope (default), wide (also the committed workareas
    /// this key reads elsewhere), or comma-separated workarea names searched
    /// alone. Needs federation on (`oml search federation`).
    #[arg(long, requires = "query")]
    sources: Option<String>,
    /// Subtree (id or name) to read; defaults to the key's workarea, or all
    /// workareas this key wrote in for --own.
    #[arg(long, conflicts_with = "artifact_id")]
    workarea: Option<String>,
    /// List this key's own rows in every state (default listing).
    #[arg(long, conflicts_with_all = ["artifact_id", "query", "all"])]
    own: bool,
    /// List promoted tips in the workarea subtree instead.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    all: bool,
    /// Filter by state: held, promoted, demoted, rejected, or erased.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    state: Option<String>,
    /// Filter by kind.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    kind: Option<String>,
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

/// The closed set of `origin.kind` (ADR-0063 d1).
const ORIGIN_KINDS: [&str; 7] = ["repo", "doc", "code", "lake", "chat", "web", "manual"];

#[derive(Args)]
struct EventsArgs {
    /// Resume after this sequence number, using the previous page's next_after_seq.
    #[arg(long)]
    after_seq: Option<u64>,
    /// Subtree to tail (id or name); defaults to the key's workarea.
    #[arg(long)]
    workarea: Option<String>,
    /// Journal events scanned per call.
    #[arg(long)]
    limit: Option<u32>,
}

#[derive(Subcommand)]
enum ReviewCommand {
    /// Claim and serve one held item for this key.
    Next,
    /// List held items this key may judge, with text, oldest first, without
    /// claiming them. Person-held dreamer keys only.
    List(ReviewListArgs),
    /// Promote one claimed serve (default) or each artifact in --ids.
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
}

#[derive(Args)]
struct ReviewListArgs {
    /// Only this workarea's subtree, by id or name.
    #[arg(long)]
    workarea: Option<String>,
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

/// `<workarea>:<role>`, or `<workarea>:<role>:node` for a grant on the node
/// alone (ADR-0044 d4), the workarea by id or name, sent as typed: the
/// server classifies and resolves it, the CLI neither (cli/reference.md
/// conventions; ADR-0026 d4). Neither an id nor a name holds a `:`, so the
/// first one splits the two; the role keeps its `:node` suffix until
/// [`grants_json`] sends it as `covers`.
fn parse_grant(s: &str) -> Result<(String, String), String> {
    let (node, role) = s
        .split_once(':')
        .ok_or_else(|| format!("{s:?} is not <workarea>:<role>"))?;
    match role.split_once(':') {
        None | Some((_, "node")) => Ok((node.to_owned(), role.to_owned())),
        Some(_) => Err(format!(
            "{s:?} is not <workarea>:<role> or <workarea>:<role>:node"
        )),
    }
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

    /// Why a source that sets one value alone is refused, naming the other.
    fn half(&self) -> String {
        let (set, unset) = if self.url.is_some() {
            (self.url_name, self.key_name)
        } else {
            (self.key_name, self.url_name)
        };
        match &self.file {
            Some(file) => format!("{file} sets {set} without {unset}"),
            None => format!("{set} is set without {unset}"),
        }
    }
}

/// Why no start source sets anything.
const NO_PAIR: &str = "no URL and API key: pass --url and --api-key, set OML_URL and OML_API_KEY, \
                       or run `oml bootstrap claim`";

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
        .ok_or_else(|| missing("no URL: pass --url or set OML_URL"))?;
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
                println!("    root wa : {}", field(&response, "root_workarea_id"));
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
            let grants = with_reader_beside_writers(&args.grants, args.write_only);
            if !cli.json {
                for (node, _) in grants.iter().filter(|g| !args.grants.contains(g)) {
                    eprintln!(
                        "adding {node}:reader beside {node}:writer \
                         (pass --write-only to send the writer grants alone)"
                    );
                }
            }
            let mut body = json!({
                "role": args.role,
                "name": args.key_name,
                "held_by": args.held_by,
                "grants": grants_json(&grants),
            });
            if let Some(wa) = &args.default_workarea {
                body["default_workarea"] = json!(wa);
            }
            if let Some(crew) = &args.crew {
                body["crew"] = json!(crew);
            }
            if let Some(user) = &args.user {
                body["user"] = json!(user);
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
            let (_, response) = client.post("/v2/keys", &body, &write_opts())?;
            if cli.json {
                print_json(&response);
            } else {
                println!("key issued — SHOWN ONCE");
                println!();
                println!("    name   : {}", field(&response, "name"));
                println!("    key id : {}", field(&response, "key_id"));
                println!("    key    : {}", field(&response, "value"));
                println!("    role   : {}", field(&response, "role"));
                println!("    default: {}", field(&response, "default_workarea_id"));
                if response["user"].is_object() {
                    println!("    user   : {}", field(&response["user"], "name"));
                }
                if let Some(grants) = response["grants"].as_array() {
                    for g in grants {
                        println!(
                            "    grant  : {} : {}{}",
                            field(g, "workarea_node_id"),
                            field(g, "role"),
                            node_suffix(g)
                        );
                    }
                }
                println!();
                println!("store this value now; the server keeps only its hash.");
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
                println!("    held by  : {}", field(&response, "held_by"));
                println!("    crew     : {}", field(&response, "crew_id"));
                println!("    user     : {}", field(&response["user"], "name"));
                println!("    default  : {}", field(&response, "default_workarea_id"));
                for g in response["grants"].as_array().into_iter().flatten() {
                    println!(
                        "    grant    : {} : {}{}",
                        field(g, "workarea_node_id"),
                        field(g, "role"),
                        node_suffix(g)
                    );
                }
                for d in response["denials"].as_array().into_iter().flatten() {
                    println!(
                        "    denial   : {} ({})",
                        field(d, "workarea_node_id"),
                        field(d, "denial_id")
                    );
                }
                println!("    epoch    : {}", response["epoch"]);
                // A revoked key's name still resolves, so the read says
                // whether the key is history (ADR-0026 d10).
                match response["revoked_at"].as_str() {
                    Some(at) => println!("    revoked  : {at}"),
                    None => println!("    revoked  : no (live)"),
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
            let mut body = json!({ "workarea": subtree });
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
                    field(&response, "workarea_node_id"),
                    response["epoch"]
                );
                println!("  denial id : {}", field(&response, "denial_id"));
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Workareas {
            command: WorkareasCommand::Create { parent, name, crew },
        } => {
            let client = resolve(cli, &stored)?;
            let mut body = json!({ "parent": parent, "name": name });
            if let Some(crew) = crew {
                body["crew"] = json!(crew);
            }
            let (_, response) = client.post("/v2/workareas", &body, &write_opts())?;
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

        Command::Workareas {
            command: WorkareasCommand::Status { command },
        } => run_workarea_status(cli, &stored, command),

        Command::Edges { command } => run_edges(cli, &stored, command),

        Command::Workareas {
            command:
                WorkareasCommand::Tree {
                    positional_root,
                    root,
                },
        } => run_workareas_tree(
            cli,
            &stored,
            positional_root
                .as_deref()
                .or(root.as_deref())
                .unwrap_or("universe"),
        ),

        Command::Workareas {
            command: WorkareasCommand::Show { workarea },
        } => {
            let client = resolve(cli, &stored)?;
            let (_, response) = client.get(&format!("/v2/workareas/{}", segment(workarea)?))?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "workarea {} ({})",
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

        Command::Workareas {
            command: WorkareasCommand::Federation { workarea, accept },
        } => {
            let client = resolve(cli, &stored)?;
            let (_, response) = client.post(
                &format!("/v2/workareas/{}/federation", segment(workarea)?),
                &json!({ "accept_federated": accept == "on" }),
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "workarea {workarea} {} federated reads",
                    if response["accept_federated"] == true {
                        "accepts"
                    } else {
                        "declines"
                    }
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Workareas {
            command: WorkareasCommand::Mode { workarea, mode },
        } => {
            let client = resolve(cli, &stored)?;
            let (_, response) = client.post(
                &format!("/v2/workareas/{}/mode", segment(workarea)?),
                &json!({ "mode": mode }),
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "workarea {} ({}) is {}",
                    field(&response, "name"),
                    field(&response, "id"),
                    field(&response, "mode"),
                );
                if mode == "in_progress" {
                    println!(
                        "readers inside now recall unreviewed memories; commit with: oml workareas mode {} standard",
                        field(&response, "name")
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Workareas {
            command:
                WorkareasCommand::Crew {
                    command: WorkareaCrewCommand::Set { workarea, crew },
                },
        } => {
            let client = resolve(cli, &stored)?;
            let (_, response) = client.post(
                &format!("/v2/workareas/{}/crew", segment(workarea)?),
                &json!({ "crew": crew }),
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "workarea {} ({}) names crew {}",
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

        Command::Write(args) => run_write(cli, &stored, &Write::of(args)),
        Command::Read(args) => run_read(cli, &stored, args),
        Command::Events(args) => run_events(cli, &stored, args),
        Command::Review { command } => run_review(cli, &stored, command),

        Command::Audit {
            command: AuditCommand::VerifyChain(args),
        } => run_verify_chain(cli, &stored, args),

        Command::Guardrails { command } => run_guardrails(cli, &stored, command),
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
            command: DemoCommand::Seed { workarea, parent },
        } => {
            let client = resolve(cli, &stored)?;
            let report = oml_cli::demo::seed(&client, workarea, parent)?;
            if cli.json {
                print_json(&json!({
                    "workarea": report.workarea,
                    "approved": report.approved,
                    "superseded": report.superseded,
                    "held": report.held,
                    "fenced_identities": report.identities,
                }));
            } else {
                println!(
                    "seeded {}: {} approved ({} of them replaced by an approved newer version), {} held",
                    report.workarea, report.approved, report.superseded, report.held
                );
                println!(
                    "fenced from the whole tree: {} (their journal entries stay)",
                    report.identities.join(", ")
                );
                println!(
                    "next: oml keys issue-agent <name> --principal crew/<name> --workarea {} --connect-code",
                    report.workarea
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
    let workarea = match command {
        GuardrailsCommand::Show { workarea } | GuardrailsCommand::Export { workarea, .. } => {
            workarea
        }
    };
    let path = match workarea {
        Some(w) => format!("/v1/guardrails?workarea={}", encode(w)),
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
    let mut body = json!({ "name": oml_cli::chat::CHAT_WORKAREA });
    if let Some(parent) = parent {
        body["parent"] = json!(parent);
    }
    let created = match client.post("/v2/workareas", &body, &write_opts()) {
        Ok((_, response)) => response,
        Err(CliError::Refused { code, .. }) if code == "name.taken" => {
            // The name is reserved for a person's key (d8), but an existing
            // `chat` is adopted only where a person's chat workarea sits:
            // directly under this key's user root, never elsewhere.
            let (_, existing) = client.get(&format!(
                "/v2/workareas/{}",
                segment(oml_cli::chat::CHAT_WORKAREA)?
            ))?;
            let me = client.whoami()?;
            let user = me["user"]["name"].as_str().unwrap_or_default();
            let path = existing["path"].as_str().unwrap_or_default();
            if let Err(why) = oml_cli::chat::adopt(path, user) {
                return Err(CliError::local(format!("oml chat init: {why}")));
            }
            println!(
                "your chat workarea {path} already exists; set OML_CHAT_WORKAREA={}",
                field(&existing, "id")
            );
            return Ok(ExitCode::SUCCESS);
        }
        Err(e) => return Err(e),
    };
    let id = field(&created, "id").to_owned();
    let mode = client.put(
        &format!("/v2/workareas/{id}/mode"),
        &json!({ "mode": "in_progress" }),
    );
    if cli.json {
        print_json(&json!({ "workarea": created, "in_progress": mode.is_ok() }));
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "created {} ({}), held facts stay inside it until you review them",
        field(&created, "path"),
        id
    );
    match mode {
        Ok(_) => {
            println!("in progress: your agents read each other's chat facts, labelled unreviewed")
        }
        Err(e) => println!(
            "not in progress ({}): each agent key reads only its own chat facts; retry with \
             `oml workareas mode {id} in_progress`",
            one_line_error(&e)
        ),
    }
    println!();
    println!("Set these where the plugin runs (or the plugin's chat_workarea option):");
    println!("  OML_CHAT_WORKAREA={id}");
    println!("  OML_CHAT_CAPTURE=1      # ask the agent to write chat facts at Stop");
    println!("  OML_TURN_RECALL=1       # optional: recall on every prompt");
    Ok(ExitCode::SUCCESS)
}

/// The chat lines only the server answers (ADR-0063 d13): whether it takes
/// `origin_kinds`, and whether the named chat workarea resolves and is the
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
    if let Some(named) = oml_cli::chat::chat_workarea(&var) {
        let user = me.and_then(|m| m["user"]["name"].as_str());
        match segment(&named).and_then(|s| client.get(&format!("/v2/workareas/{s}"))) {
            Ok((_, node)) => out.extend(oml_cli::chat::chat_scope_line(node["path"].as_str(), user)),
            Err(_) => out.push(format!(
                "warn chat: {} names {named}, which this key cannot resolve; run oml chat init and use the id it prints",
                oml_cli::chat::WORKAREA_VAR
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

/// A crew record as one line: its name and id, or `—` for none.
fn crew_line(crew: &Value) -> String {
    if crew.is_object() {
        format!("{} ({})", field(crew, "name"), field(crew, "crew_id"))
    } else {
        "—".to_owned()
    }
}

/// The issuance body's `grants` (openapi.yaml `GrantRequest`), each
/// workarea as the operator spelled it.
fn grants_json(grants: &[(String, String)]) -> Vec<Value> {
    grants
        .iter()
        .map(|(node, role)| match role.split_once(':') {
            // A `:node` suffix is a grant on the node alone (ADR-0044 d4).
            Some((role, covers)) => json!({ "workarea": node, "role": role, "covers": covers }),
            None => json!({ "workarea": node, "role": role }),
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
        PublicServerConsent, authorize_public_server, record_public_server_consent,
    };
    let consent =
        authorize_public_server(client.server(), source, explicit).map_err(CliError::Local)?;
    let scope = match consent {
        PublicServerConsent::FirstOptIn => "for this clone",
        PublicServerConsent::OneTime => "for this run only; the source is outside a git clone",
        PublicServerConsent::NotRequired | PublicServerConsent::Recorded => return Ok(()),
    };
    eprintln!(
        "warn: public-server sync allowed {scope} for {}: its admins can read submitted memories; \
         supersession or retirement does not erase submitted text or anchors",
        client.server()
    );
    if consent == PublicServerConsent::FirstOptIn {
        record_public_server_consent(source).map_err(CliError::Local)?;
    }
    Ok(())
}

/// `oml sync`: refuses the public server unless allowed, reads the source
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
        workarea: args.workarea.clone(),
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
        workarea: args.workarea.clone(),
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
    workarea: Option<&'a str>,
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
}

impl<'a> Write<'a> {
    fn of(args: &'a WriteArgs) -> Self {
        Self {
            text: &args.text,
            claim_key: args.claim_key.as_deref(),
            kind: args.kind.as_deref(),
            derives_from: &args.derives_from,
            workarea: args.workarea.as_deref(),
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
        }
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
        ("workarea", write.workarea),
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
        println!("  workarea  : {}", field(&response, "workarea_id"));
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

/// `oml read`, in `oml_read`'s three modes (ADR-0039 d4): with an artifact
/// id one `GET /v1/memories/{artifact_id}`, with `--query` one `POST
/// /v1/memories/recall` with the text, and with neither one `GET
/// /v1/memories`, `own=true` unless `--all`. A flag outside the mode its
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
        if let Some(workarea) = &args.workarea {
            body["workarea"] = json!(workarea);
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
        let mut params = vec![("own", (!args.all).to_string())];
        for (name, value) in [
            ("workarea", args.workarea.as_ref()),
            ("state", args.state.as_ref()),
            ("kind", args.kind.as_ref()),
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
            field(&response, "workarea_path")
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
        for item in response["items"].as_array().into_iter().flatten() {
            println!(
                "{}  {}  v{}  {}  {}  {}  {}",
                field(item, "artifact_id"),
                field(item, "claim_key"),
                item["version"],
                field(item, "state"),
                field(item, "kind"),
                field(item, "workarea_path"),
                field(item, "created_at")
            );
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
    if let Some(workarea) = &args.workarea {
        params.push(("workarea", workarea.clone()));
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
        for event in page["items"].as_array().into_iter().flatten() {
            println!(
                "{}  {}  {}  {}",
                event["seq"],
                field(event, "event_type"),
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

/// `oml workareas status`: propose, ratify and the pending list over the
/// transition routes and the workarea read (ADR-0046 d3).
fn run_workarea_status(
    cli: &Cli,
    stored: &Credentials,
    command: &WorkareaStatusCommand,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    match command {
        WorkareaStatusCommand::Propose { workarea, to } => {
            let (_, response) = client.post(
                &format!("/v2/workareas/{}/transitions", segment(workarea)?),
                &json!({ "to": to }),
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "proposed {} -> {} on {workarea}: transition {} waits for a second identity",
                    field(&response, "from"),
                    field(&response, "to"),
                    field(&response, "transition_id")
                );
            }
        }
        WorkareaStatusCommand::Ratify {
            workarea,
            transition_id,
        } => {
            let (_, response) = client.post(
                &format!(
                    "/v2/workareas/{}/transitions/{}/ratify",
                    segment(workarea)?,
                    segment(transition_id)?
                ),
                &json!({}),
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!("ratified: {workarea} is now {}", field(&response, "to"));
            }
        }
        WorkareaStatusCommand::Pending { workarea } => {
            let (_, node) = client.get(&format!("/v2/workareas/{}", segment(workarea)?))?;
            let pending = json!({
                "workarea_id": node["id"],
                "status": node["status"],
                "policy": node["policy"],
                "pending_transitions": node["pending_transitions"],
            });
            if cli.json {
                print_json(&pending);
            } else {
                println!("{workarea} is {}", field(&node, "status"));
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
                EdgeProposeCommand::Brief { artifact, workarea } => json!({
                    "kind": "brief", "from_artifact_id": artifact, "to_workarea_id": workarea,
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
            workarea,
            artifact,
            limit,
            cursor,
        } => {
            let mut params = vec![("kind", kind.clone())];
            if let Some(workarea) = workarea {
                params.push(("workarea_id", workarea.clone()));
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
                .or_else(|| e["to_workarea_id"].as_str())
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
        ReviewCommand::Next => run_review_next(cli, stored),
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
/// serves for this key; 204 when nothing in scope waits.
fn run_review_next(cli: &Cli, stored: &Credentials) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let (status, response) = client.post("/v2/review/next", &json!({}), &write_opts())?;
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
            "  artifact  {} @ v{}   kind: {}   writer: {}",
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
        ("workarea", &args.workarea),
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
                field(item, "workarea_path"),
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
            format!(
                "{repo} @ {} {}{fragment}",
                &rev[..rev.len().min(12)],
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

/// One `POST /v2/keys` for a writer key named `<key-name>`, with writer and
/// reader grants on the workarea, that workarea as its default and the
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
    // With --user the workarea defaults to the user's root, which the
    // server resolves in that user's namespace (ADR-0044 d11).
    let workarea = match (&args.workarea, &args.user) {
        (Some(workarea), _) => workarea.clone(),
        (None, Some(user)) => user.clone(),
        (None, None) => {
            return Err(CliError::local(
                "refused: --workarea is required without --user; nothing was sent",
            ));
        }
    };
    let client = resolve(cli, stored)?;
    let key_file = match &args.out {
        Some(path) => Some(KeyFile::create(path).map_err(CliError::Local)?),
        None => None,
    };
    let grants =
        with_reader_beside_writers(&[(workarea.clone(), "writer".to_owned())], args.write_only);
    let mut body = json!({
        "role": "writer",
        "name": args.key_name,
        "held_by": args.held_by,
        "principal": args.principal,
        "default_workarea": workarea,
        "grants": grants_json(&grants),
    });
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
                        "{}:{}{}",
                        field(g, "workarea_node_id"),
                        field(g, "role"),
                        if g["covers"] == "node" { ":node" } else { "" }
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    eprintln!(
        "key {key_name} ({key_id}) issued to {} (writer, held by {}): {}; default workarea {}",
        field(&response, "principal"),
        args.held_by,
        granted.join(", "),
        field(&response, "default_workarea_id"),
    );
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
// oml workareas tree (ADR-0026 d15)
// ---------------------------------------------------------------------------

/// The tree from `root` down, by id or name, the default `universe`
/// (ADR-0026 d6): every page of `GET /v2/workareas/{workarea_id}/tree`, in
/// the server's path order. Under an admin key the read answers by the key
/// role, for every workarea of the tenant and structure only; under a reader
/// key it answers within reader effective access, every node outside it
/// absent (http-api.md workareas; ADR-0026 d15). `--json` prints the pages'
/// items as one page (cli/reference.md `oml workareas`).
fn run_workareas_tree(cli: &Cli, stored: &Credentials, root: &str) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let tree = format!("/v2/workareas/{}/tree", segment(root)?);
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

fn run_admin(
    cli: &Cli,
    stored: &Credentials,
    command: &AdminCommand,
) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let (memory, query, workarea, state) = match command {
        AdminCommand::Rescan { record_held, rates } => {
            return run_rescan(cli, &client, *record_held, *rates);
        }
        AdminCommand::Read {
            memory,
            query,
            workarea,
            state,
        } => (memory, query, workarea, state),
    };
    let line = |m: &Value| {
        format!(
            "{}  {}  {}  {}  {}",
            field(m, "artifact_id"),
            field(m, "state"),
            field(m, "workarea_path"),
            field(m, "key"),
            field(m, "text").replace('\n', " ")
        )
    };
    if let Some(memory) = memory {
        let (_, read) = client.get(&format!("/v2/admin/memories/{}", segment(memory)?))?;
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
    let mut body = json!({});
    if let Some(q) = query {
        body["query"] = json!(q);
    }
    if let Some(w) = workarea {
        body["workarea"] = json!(w);
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
                client
                    .post("/v2/federation", &Value::Object(body), &write_opts())?
                    .1
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
        WebCommand::Allow { email, user, admin } => {
            let body = json!({ "email": email, "user": user, "admin": admin });
            let (_, entry) = client.post("/v2/web/emails", &body, &write_opts())?;
            if cli.json {
                print_json(&entry);
            } else {
                println!("allowed {}", entry_line(&entry));
            }
        }
        WebCommand::Remove { email } => {
            let body = json!({ "email": email });
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
            "logged in to {} as key {} ({}): role {}, tenant {}, user root {}, default workarea {}",
            client.server(),
            field(&me, "name"),
            field(&me, "key_id"),
            field(&me, "role"),
            field(&me["tenant"], "name"),
            user_root_line(&me),
            field(&me["default_workarea"], "path"),
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
/// --wait`, the claim of the token the lead logged, the crew workarea, an
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
        ],
        Some(token),
    )?;
    admin.json(&["workareas", "create", "universe", "crew"], None)?;
    let agent_file = keys.join("agent");
    let agent_shown = agent_file.display().to_string();
    admin.text(&[
        "keys",
        "issue-agent",
        "quickstart-agent",
        "--principal",
        "crew/agent",
        "--workarea",
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
                "dreamer",
                "--held-by",
                "human",
                "--principal",
                "crew/reviewer",
                "--default-workarea",
                "crew",
                "--grant",
                "crew:dreamer",
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

/// The pair `oml setup claude-code` hands the plugin, and whether its URL is
/// the default: `--key-file`'s key with the `--url` flag, a flag source of
/// its own, or the pair as [`resolve`] takes it (ADR-0039 d15, ADR-0041 d7
/// step 1). A key named without a URL, by either form, is paired with
/// [`oml_cli::setup::DEFAULT_URL`] and the verb says so on stderr before
/// anything is sent; a URL named without a key is refused (ADR-0043 d4).
fn setup_pair(
    cli: &Cli,
    stored: &Credentials,
    key_file: Option<&std::path::Path>,
) -> Result<(Client, String, bool), CliError> {
    use oml_cli::setup::url_beside_key;
    let beside = |named: Option<&String>| {
        let (url, said) = url_beside_key(named.map(String::as_str));
        if let Some(said) = said {
            eprintln!("{said}");
        }
        (url, said.is_some())
    };
    let Some(path) = key_file else {
        return match first_source(cli, stored) {
            Some(Source {
                url,
                key: Some(key),
                ..
            }) => {
                let (url, defaulted) = beside(url);
                Ok((Client::new(url, Some(key.clone())), key.clone(), defaulted))
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
    let (url, defaulted) = beside(cli.start.flag_url.as_ref());
    Ok((
        Client::new(url, Some(key.to_owned())),
        key.to_owned(),
        defaulted,
    ))
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
    let (client, key, defaulted) = setup_pair(cli, stored, args.key_file.as_deref())?;
    let me = client.whoami().inspect_err(|_| {
        if defaulted {
            eprintln!("{}", setup::DEFAULT_REFUSED);
        }
    })?;
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

/// Whether this key may review in bulk, for a dreamer key only (ADR-0055
/// d1, d13): a key a person holds may; one held by an agent or a service is
/// refused, which a person holding such a key otherwise learns mid-import,
/// so the line warns and names the fix.
fn review_doctor(me: Option<&Value>) -> Option<String> {
    let me = me?;
    if me["role"] != "dreamer" {
        return None;
    }
    Some(match me["held_by"].as_str() {
        Some("human") => "ok bulk review: this key lists and judges held items in bulk (oml review list, oml review approve --ids)".to_owned(),
        held_by => format!(
            "warn bulk review: this key is held by {}, so it cannot review in bulk \
             (access.verb_ceiling, bulk_review_is_human_only); use a dreamer key issued \
             --held-by human or the admin page",
            held_by.unwrap_or("unknown")
        ),
    })
}

/// The in-progress lines (ADR-0030 d14): whether the default workarea lies
/// inside an in-progress workarea, so recall serves unreviewed memories
/// there; for a person's dreamer key, that it may set the mode and how many
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
            "ok in progress: the default workarea is inside {root}; recall includes labelled \
             unreviewed memories, which stay inside until a person commits them"
        )),
        None if me["profile"] == "curator" => lines.push(
            "warn curator: the default workarea is not in progress; curator keys act only \
             inside in-progress workareas (access.verb_ceiling, curator_outside_in_progress)"
                .to_owned(),
        ),
        None => {}
    }
    if me["role"] == "dreamer" && me["held_by"] == "human" && me["profile"].is_null() {
        lines.push(
            "ok mode: set workarea mode with `oml workareas mode <workarea> \
             in_progress|standard`"
                .to_owned(),
        );
        if let (Some(root), Some(n)) = (root, held_blocking)
            && n > 0
        {
            lines.push(format!(
                "warn commit: {n} held item(s) block committing {root}; review them with \
                 `oml review list --workarea {root}` (workarea.held_remaining)"
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
    // that cannot is warned about (ADR-0044 d14).
    let mut reads_universe: Option<bool> = None;
    let mut admin_emails: Option<(usize, Vec<String>)> = None;
    let mut signup_overview: Option<Value> = None;
    let mut scan_backlog: Option<Value> = None;
    let mut default_node: Option<Value> = None;
    let mut waiting_edges: Option<usize> = None;
    let mut search_settings: Option<Value> = None;
    let mut federation_settings: Option<Value> = None;
    // The chat workarea's path and whether the server takes origin_kinds
    // (ADR-0063 d13).
    let mut chat_server: Vec<String> = Vec::new();
    let mut held_blocking: Option<usize> = None;
    if let Some(url) = source.as_ref().and_then(|s| s.url) {
        let keyless = Client::new(url.clone(), None);
        health = Some(keyless.probe("/healthz")?);
        ready = Some(keyless.probe("/readyz")?);
        if let Some(key) = source.as_ref().and_then(|s| s.key) {
            let client = Client::new(url.clone(), Some(key.clone()));
            me = Some(client.whoami());
            chat_server = chat_doctor(&client, me.as_ref().and_then(|m| m.as_ref().ok()));
            if let Some(Ok(v)) = &me
                && v["user"].is_object()
            {
                reads_universe = Some(match client.get("/v2/workareas/universe") {
                    Ok(_) => true,
                    Err(CliError::Refused { .. }) => false,
                    Err(other) => return Err(other),
                });
            }
            // The default workarea's brief and waiting transitions, which a
            // session start and a ratifier depend on (ADR-0046 d9).
            if let Some(Ok(v)) = &me
                && let Some(id) = v["default_workarea"]["id"].as_str()
                && let Ok(segment) = segment(id)
            {
                default_node = client
                    .get(&format!("/v2/workareas/{segment}"))
                    .ok()
                    .map(|(_, n)| n);
                // What a person's reviewer key sees held under the
                // in-progress root, one page (ADR-0030 d14).
                if v["role"] == "dreamer"
                    && v["held_by"] == "human"
                    && let Some(root) = default_node
                        .as_ref()
                        .and_then(|n| n["in_progress_root"]["id"].as_str())
                {
                    held_blocking = client
                        .get(&format!("/v2/review/held?workarea={root}&limit=200"))
                        .ok()
                        .and_then(|(_, page)| page["items"].as_array().map(Vec::len));
                }
                // The claim edges there waiting for a second key, one page
                // (ADR-0057 d17; ADR-0046 d4).
                waiting_edges = client
                    .get(&format!(
                        "/v2/edges?kind=claim&workarea_id={segment}&limit=200"
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
                search_settings = client.get("/v2/search").ok().map(|(_, v)| v);
                // The tenant's federation policy and its wide keys
                // (ADR-0072 d12, Consequences).
                federation_settings = client.get("/v2/federation").ok().map(|(_, v)| v);
                // One event's window, read for the counts it carries
                // (ADR-0074 d28).
                scan_backlog = client.get("/v2/admin/rescan?limit=1").ok().map(|(_, v)| v);
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
    let in_progress = in_progress_doctor(
        me.as_ref().and_then(|m| m.as_ref().ok()),
        default_node.as_ref(),
        held_blocking,
    );
    let in_progress_warn = in_progress.iter().any(|line| line.starts_with("warn"));
    let telemetry = telemetry_doctor(scrape.as_ref());
    let mut scan = scan_doctor(ready.as_ref(), scrape.as_ref());
    scan.extend(scan_backlog_doctor(scan_backlog.as_ref()));
    let scan_warn = scan.iter().any(|line| line.starts_with("warn"));
    let telemetry_warn = telemetry.iter().any(|line| line.starts_with("warn"));
    let handlers = handlers_doctor(scrape.as_ref());
    let handlers_warn = handlers.iter().any(|line| line.starts_with("warn"));
    // The sync failures of ADR-0057 d17 that the clone alone answers; the
    // stale count and the waiting edges are the sources lines'.
    let sync = oml_cli::sync::doctor::lines(
        source.as_ref().and_then(|s| s.url.map(String::as_str)),
        &std::env::current_dir().unwrap_or_default(),
    );
    let sync_warn = sync.iter().any(|line| line.starts_with("warn"));
    // The chat hooks' local state (ADR-0063 d13).
    let mut chat = oml_cli::chat::doctor_lines(&|name: &str| std::env::var(name).ok());
    chat.extend(chat_server);
    let chat_warn = chat.iter().any(|line| line.starts_with("warn"));
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
    let probes_ok = matches!((&health, &ready), (Some((200, _)), Some((200, _))));
    let whoami_ok = !matches!(me, Some(Err(_)));
    let review_warn = review.as_ref().is_some_and(|line| line.starts_with("warn"));
    let outcome = if !probes_ok || !whoami_ok || !oauth_ok {
        "fail"
    } else if me.is_none()
        || reads_universe == Some(false)
        || web_warn
        || search_warn
        || review_warn
        || telemetry_warn
        || handlers_warn
        || scan_warn
        || sync_warn
        || chat_warn
        || sources_warn
        || in_progress_warn
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
            "default_workarea": v["default_workarea"]["path"],
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
            "default_workarea": default_node.as_ref().map(|n| json!({
                "path": n["path"],
                "brief_items": n["brief"]["items"].as_array().map_or(0, Vec::len),
                "brief_truncated": n["brief"]["budget"]["truncated"],
                "pending_transitions": n["pending_transitions"].as_array().map_or(0, Vec::len),
            })),
            "oauth_discovery": oauth,
            "web": web,
            "search": search,
            "review": review,
            "sync": sync,
            "chat": chat,
            "telemetry": telemetry,
            "handlers": handlers,
            "scan": scan,
            "sources": sources,
            "in_progress": in_progress,
            "client_version": env!("CARGO_PKG_VERSION"),
            "server_version": server_version(ready.as_ref()),
            "verdict": outcome,
            "next": next,
        }));
    } else {
        println!("url         : {}", url_source.as_deref().unwrap_or("unset"));
        println!("api key     : {}", key_source.as_deref().unwrap_or("unset"));
        match &health {
            Some((status, body)) => println!(
                "healthz     : {} ({})",
                verdict(*status == 200),
                field(body, "status")
            ),
            None => println!("healthz     : not probed (no URL)"),
        }
        match &ready {
            Some((status, body)) => {
                println!("readyz      : {}", verdict(*status == 200));
                if let Some(checks) = body["checks"].as_object() {
                    for (name, state) in checks {
                        println!("  {name:<10}: {}", state.as_str().unwrap_or("—"));
                    }
                }
            }
            None => println!("readyz      : not probed (no URL)"),
        }
        match &me {
            Some(Ok(_)) => println!(
                "whoami      : role {}, tenant {}, default workarea {}",
                whoami["role"].as_str().unwrap_or("—"),
                whoami["tenant"].as_str().unwrap_or("—"),
                whoami["default_workarea"].as_str().unwrap_or("—"),
            ),
            Some(Err(e)) => println!("whoami      : fail ({})", refusal(e)),
            None => println!("whoami      : not called (no key with this URL)"),
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
        if let Some(n) = &default_node {
            let items = n["brief"]["items"].as_array().map_or(0, Vec::len);
            if items == 0 {
                println!(
                    "brief       : empty: no ratified brief edge on {}; a dreamer proposes one with oml edges propose brief",
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
                    "transitions : {waiting} waiting for a second identity; oml workareas status pending {}",
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
            None => println!("oauth       : not probed (no URL)"),
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
        for line in &scan {
            println!("scan        : {line}");
        }
        for line in &sync {
            println!("sync        : {line}");
        }
        for line in &chat {
            println!("chat        : {line}");
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
            (None, None) => println!("server      : not probed (no URL)"),
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
            "warn key {} reads {} trusted workareas outside its scopes, above 1,000: each recall of a federated tenant reads their counts, so its statistics read grows with them",
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
/// authors' alone in in-progress workareas until `--record-held` reads
/// them, and every stored version an older rule set scanned.
fn scan_backlog_doctor(report: Option<&Value>) -> Vec<String> {
    let Some(r) = report else {
        return vec![
            "scan_held_unrescanned, scan_rescan_due: not read; an admin key reads them".to_owned(),
        ];
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
                "warn scan_held_unrescanned: {held} held version(s) were scanned under an older rule set than {version}, and stay their authors' alone in in-progress workareas; run oml admin rescan --record-held"
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

/// The scan doctor lines (ADR-0065 d11): the rule set `/readyz` names, how
/// many rules it loaded and whether the personal-data rules are on; and,
/// where `OML_METRICS_URL` names the metrics listener, the findings since
/// start by action and the rules that fired most. Counts only: no finding
/// carries what it matched.
fn scan_doctor(ready: Option<&(u16, Value)>, scrape: Option<&Scrape>) -> Vec<String> {
    let Some((_, body)) = ready else {
        return vec!["not probed (no URL)".to_owned()];
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
    if n("workarea_drift") > 0 || n("term_drift") > 0 {
        lines.push(format!(
            "warn search: the postings counts disagree: {} workarea count{}, {} of {} sampled word counts; a defect, which ranks by wrong frequencies: {refill}, and report it",
            n("workarea_drift"),
            if n("workarea_drift") == 1 { "" } else { "s" },
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
    let faulty = h["partition"] != Value::Bool(true)
        || h["similar_partition"] == Value::Bool(false)
        || n("similar_unfilled") > 0
        || n("similar_drift") > 0
        || n("unfilled") > 0
        || n("served_behind") > 0
        || n("workarea_drift") > 0
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

/// Open sign-up's doctor lines (ADR-0068 d5, d6, d9, d11), from
/// `GET /v2/web/users` read by an admin key: the people at or above 90% of
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
        for quota in ["memories", "keys", "connections"] {
            let used = u["usage"][quota].as_u64().unwrap_or(0);
            let limit = u["quotas"][quota].as_u64().unwrap_or(0);
            if limit > 0 && used.saturating_mul(10) >= limit.saturating_mul(9) {
                *near.entry(quota).or_default() += 1;
                hit = true;
            }
        }
        people += usize::from(hit);
    }
    if people > 0 {
        let by: Vec<String> = near.iter().map(|(q, n)| format!("{q}: {n}")).collect();
        lines.push(format!(
            "warn quota: {people} users at or above 90% of a quota ({})",
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
/// anchor; the served versions of the key's default workarea that read
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
        let path = node["path"].as_str().unwrap_or("the default workarea");
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
            "{waiting} claim edge(s) in the default workarea wait for a second key to ratify them"
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
    use super::*;

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
            scan_backlog_doctor(Some(&report))[2],
            "warn scan_findings_unlisted: 2 held version(s) carry a scan finding the findings list lacks, written by a server older than migration 0043; the review card shows only the stored summary for them; run oml admin rescan --record-held"
        );
        let clear = json!({
            "scanner_version": "oml-scan/1.2", "held_unrescanned": 0, "rescan_due": 0,
            "findings_unlisted": 0,
        });
        assert_eq!(
            scan_backlog_doctor(Some(&clear))[2],
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
            scan_backlog_doctor(Some(&due)),
            vec![
                "warn scan_held_unrescanned: 4 held version(s) were scanned under an older rule set than oml-scan/1.2, and stay their authors' alone in in-progress workareas; run oml admin rescan --record-held".to_owned(),
                "warn scan_rescan_due: 9 stored version(s) were scanned under an older rule set than oml-scan/1.2; oml admin rescan reports what it finds in them".to_owned(),
            ]
        );
        let clear = json!({
            "scanner_version": "oml-scan/1.2", "held_unrescanned": 0, "rescan_due": 0,
        });
        assert_eq!(
            scan_backlog_doctor(Some(&clear)),
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
    fn doctor_names_people_near_a_quota_pending_deletions_and_a_shared_address_limit() {
        let overview = serde_json::json!({
            "mode": "open",
            "address_limit_shared": true,
            "users": [
                {"user": "ua", "under_quotas": true, "disabled": false,
                 "usage": {"memories": 950, "keys": 1, "connections": 0},
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
                "warn quota: 2 users at or above 90% of a quota (keys: 1, memories: 1)",
                "warn signup: 1 deletion requests pending, oldest requested 2026-11-06T10:00:00.000000Z; close each on /web/admin/users",
            ]
        );
        let quiet = serde_json::json!({"mode": "allowlist", "address_limit_shared": true, "users": [], "deletions": []});
        assert!(signup_doctor(Some(&quiet)).is_empty());
        assert!(signup_doctor(None).is_empty());
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
