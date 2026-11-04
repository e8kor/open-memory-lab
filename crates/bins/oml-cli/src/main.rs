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
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgGroup, Args, Parser, Subcommand};
use serde_json::{Value, json};
use uuid::Uuid;

use oml_cli::client::{CallOpts, CliError, Client, encode, new_idempotency_key, segment};
use oml_cli::creds::{self, Credentials};
use oml_cli::hook;
use oml_cli::issue::{
    KeyFile, configure_line, connect_code_printed, sh_quote, with_reader_beside_writers,
};
use oml_cli::verify::parse_verifying_key;

#[derive(Parser)]
#[command(
    name = "oml",
    version,
    about = "Open Memory Lab administration — a lab client of the same HTTP API every agent speaks (ADR-0015)."
)]
struct Cli {
    /// The server's URL, without /mcp. One pair with --api-key: the pair
    /// comes from the flags, else the environment, else the credentials
    /// file, and a source setting one alone is refused (ADR-0039 d15).
    #[arg(long, global = true, env = "OML_URL")]
    url: Option<String>,
    /// The API key value, paired with --url. argv is visible to other
    /// processes, so prefer OML_API_KEY or the credentials file.
    #[arg(
        long = "api-key",
        global = true,
        env = "OML_API_KEY",
        hide_env_values = true
    )]
    api_key: Option<String>,
    /// The v0.3.0 spelling of --url, read through v0.4.x (ADR-0039 d12).
    #[arg(long, global = true, hide = true)]
    server: Option<String>,
    /// The v0.3.0 spelling of --api-key, read through v0.4.x (ADR-0039 d12).
    #[arg(long, global = true, hide = true)]
    key: Option<String>,
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
    /// Splits `cli`'s `--url` and `--api-key` by where clap took each from;
    /// the hidden `--server` and `--key` count as the flags they were
    /// renamed to.
    fn of(cli: &Cli, matches: &clap::ArgMatches) -> Self {
        use clap::parser::ValueSource;
        let set = |v: &Option<String>| v.clone().filter(|s| !s.is_empty());
        let from = |id: &str, source: ValueSource| matches.value_source(id) == Some(source);
        let pick = |id: &str, v: &Option<String>, source: ValueSource| {
            if from(id, source) { set(v) } else { None }
        };
        Self {
            flag_url: pick("url", &cli.url, ValueSource::CommandLine).or_else(|| set(&cli.server)),
            flag_key: pick("api_key", &cli.api_key, ValueSource::CommandLine)
                .or_else(|| set(&cli.key)),
            env_url: pick("url", &cli.url, ValueSource::EnvVariable),
            env_key: pick("api_key", &cli.api_key, ValueSource::EnvVariable),
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// The bootstrap ceremony: exchange the enrollment token for a key.
    Bootstrap {
        #[command(subcommand)]
        command: BootstrapCommand,
    },
    /// Tenant verbs (operator tier).
    Tenants {
        #[command(subcommand)]
        command: TenantsCommand,
    },
    /// Key issuance (show-once ceremony).
    Keys {
        #[command(subcommand)]
        command: KeysCommand,
    },
    /// Global per-key denials: deny-wins across all of a key's grants.
    Deny {
        #[command(subcommand)]
        command: DenyCommand,
    },
    /// Workarea tree verbs.
    Workareas {
        #[command(subcommand)]
        command: WorkareasCommand,
    },
    /// Crew records: named, recorded, not operated (admin key role).
    Crews {
        #[command(subcommand)]
        command: CrewsCommand,
    },
    /// Write a claim, held until reviewed: a submission, or with --replaces
    /// a new version of a claim or its retirement (ADR-0039 d3, d10).
    Write(WriteArgs),
    /// Read memories: fetch one by artifact id, recall with --query, or
    /// list with neither (ADR-0039 d4, d10).
    Read(ReadArgs),
    /// Tail the journal's events within the key's reader access, digests
    /// only (GET /v2/events; ADR-0039 d10).
    Events(EventsArgs),
    /// The review ceremony.
    Review {
        #[command(subcommand)]
        command: ReviewCommand,
    },
    /// The v0.3.0 name of `oml write`, hidden until v0.5.0 (ADR-0039 d12).
    #[command(hide = true)]
    Submit(SubmitArgs),
    /// The v0.3.0 name of `oml read --query`, hidden until v0.5.0
    /// (ADR-0039 d12).
    #[command(hide = true)]
    Recall(RecallArgs),
    /// The v0.3.0 name of `oml write --replaces`, hidden until v0.5.0
    /// (ADR-0039 d12).
    #[command(hide = true)]
    Supersede(SupersedeArgs),
    /// Evidence verbs.
    Audit {
        #[command(subcommand)]
        command: AuditCommand,
    },
    /// Read an API key without echo, from the terminal or from stdin when
    /// stdin is not one, check it through GET /v2/whoami, and only then
    /// store the URL and the key, nothing else, owner-only (ADR-0039 d15).
    Login {
        /// The server's URL, without /mcp.
        // Not `url`: clap would merge that id with the global --url.
        #[arg(value_name = "URL")]
        target: String,
    },
    /// Each health signal with a verdict; exit 1 on any fail.
    Doctor,
    /// Claude Code hooks of the agent plugin, run under the agent's own key.
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
}

#[derive(Subcommand)]
enum HookCommand {
    /// Print the SessionStart context from the plugin's url and api_key
    /// options, else OML_URL and OML_API_KEY — steering, this key's recent
    /// submissions, the matching memories — or nothing; always exits 0
    /// (ADR-0029, ADR-0040 d3).
    SessionStart,
    /// Print memories matching a failed Bash command's error, from the
    /// plugin's url and api_key options, else OML_URL and OML_API_KEY, only
    /// with OML_FAILURE_RECALL=1; always exits 0 (ADR-0038, ADR-0040 d3).
    ToolFailure,
    /// Delete this session's failure-recall cache; prints nothing and
    /// always exits 0 (ADR-0038).
    SessionEnd,
}

#[derive(Subcommand)]
enum BootstrapCommand {
    /// Exchange the single-use enrollment token for the first admin key and
    /// store the URL and the key, nothing else, owner-only (0600).
    Claim {
        /// The name of the key the claim mints, unique in its namespace and
        /// never released (ADR-0026 d9, d12).
        key_name: String,
        /// Read the enrollment token the lead printed at first boot from
        /// stdin, without echo on a terminal, so it never reaches argv or
        /// shell history; required, since --token is gone (ADR-0039 d10).
        #[arg(long, required = true)]
        token_stdin: bool,
        /// Name for the tenant the ceremony creates.
        #[arg(long)]
        tenant_name: Option<String>,
        /// Principal recorded on the first admin key.
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
    /// Issue a key; the value is printed exactly once.
    Issue(KeysIssueArgs),
    /// Issue the one key the agent plugin needs, in one POST /v2/keys, and
    /// print the OML_URL and OML_API_KEY export lines, and with --out the
    /// claude plugin configure line that reads the file (ADR-0028 d1,
    /// ADR-0040 d4).
    IssueAgent(IssueAgentArgs),
    /// One key, by id or name, a revoked key's included: grants, denials,
    /// crew and epoch, never its value.
    Show {
        /// The key's id or name.
        // Not `key`: clap would merge that id with the global --key.
        #[arg(value_name = "KEY")]
        key_ref: String,
    },
}

#[derive(Args)]
struct KeysIssueArgs {
    /// The key's name, unique in the tenant and never released; distinct
    /// from its principal and label (ADR-0026 d9, d12).
    key_name: String,
    /// reader | writer | dreamer | admin
    #[arg(long)]
    role: String,
    /// agent | human | service
    #[arg(long, default_value = "agent")]
    held_by: String,
    /// Workarea (id or name) the provider ring targets when a request names
    /// none; some grant must cover it. Omitted, the first grant's node.
    #[arg(long)]
    default_workarea: Option<String>,
    /// Grant as <workarea>:<role>, the workarea by id or name; repeatable. A
    /// writer grant brings a reader grant on the same workarea unless
    /// --write-only.
    #[arg(long = "grant", value_parser = parse_grant)]
    grants: Vec<(String, String)>,
    /// The key's crew record, by id or name, fixed for the key's life.
    #[arg(long)]
    crew: Option<String>,
    /// Send the writer grants alone, with no reader grant beside them
    /// (ADR-0028 d2).
    #[arg(long)]
    write_only: bool,
    /// Principal the key acts as.
    #[arg(long)]
    principal: Option<String>,
    #[arg(long)]
    label: Option<String>,
}

#[derive(Args)]
struct IssueAgentArgs {
    /// The key's name, unique in the tenant and never released; never
    /// derived from the principal (ADR-0026 d9, d12).
    key_name: String,
    /// Principal the key acts as; give each agent its own.
    #[arg(long)]
    principal: String,
    /// The workarea, by id or name, the key writes and recalls on, and its
    /// default.
    #[arg(long)]
    workarea: String,
    /// The key's crew record, by id or name, fixed for the key's life
    /// (ADR-0026 d12).
    #[arg(long)]
    crew: Option<String>,
    /// Who holds the key.
    #[arg(long, default_value = "agent", value_parser = ["agent", "human"])]
    held_by: String,
    /// Send the writer grant alone, with no reader grant beside it
    /// (ADR-0028 d2).
    #[arg(long)]
    write_only: bool,
    /// Write the key value to this new file (mode 0600, never overwritten)
    /// and print a line that reads it back instead of the value.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Issue a connector key for a claude.ai custom connector: no value, and
    /// a single-use connect code printed in its place (ADR-0031 d7).
    #[arg(long)]
    connect_code: bool,
}

#[derive(Subcommand)]
enum DenyCommand {
    /// Create or extend the key's global deny object over a subtree. Distinct
    /// from a per-grant deny; journalled as key.denied.
    Set {
        /// The key the denial binds, by id or name.
        // Not `key`: clap would merge that id with the global --key.
        #[arg(value_name = "KEY")]
        key_ref: String,
        /// The workarea, by id or name, whose subtree the key no longer
        /// reaches.
        subtree: String,
        /// Why; the server accepts it and journals no reason.
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand)]
enum WorkareasCommand {
    /// Create a child workarea in development status.
    Create {
        /// The parent workarea, by id or name.
        parent: String,
        /// The new workarea's name, unique in the tenant and never changed.
        name: String,
        /// The crew record the workarea names, by id or name.
        #[arg(long)]
        crew: Option<String>,
    },
    /// The tree from a workarea down, with each node's path, id, status
    /// and crew: every page of one tree read.
    Tree {
        /// The workarea the listing starts at, by id or name.
        #[arg(long, default_value = "universe")]
        root: String,
    },
    /// One workarea, by id or name: path, status, crew, children.
    Show {
        /// The workarea's id or name.
        workarea: String,
    },
    /// The crew record a workarea names (admin key role).
    Crew {
        #[command(subcommand)]
        command: WorkareaCrewCommand,
    },
}

#[derive(Subcommand)]
enum WorkareaCrewCommand {
    /// Point the workarea at a crew record (workarea.crew_updated); allowed
    /// on validated workareas.
    Set {
        /// The workarea, by id or name.
        workarea: String,
        /// The crew record, by id or name.
        #[arg(long)]
        crew: String,
    },
}

#[derive(Subcommand)]
enum CrewsCommand {
    /// Create a crew record (crew.created); its name never changes.
    Create {
        /// The crew's name, unique in the tenant.
        name: String,
        #[arg(long)]
        charter_uri: Option<String>,
        #[arg(long)]
        external_manager: Option<String>,
    },
    /// Update a crew record's charter URI or external manager
    /// (crew.updated); at least one flag, never the name.
    Update(CrewsUpdateArgs),
    /// The tenant's crew records.
    List,
    /// One crew record, by id or name.
    Show {
        /// The crew's id or name.
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
struct WriteArgs {
    /// The claim text.
    text: String,
    /// Claim key; omitted, the server derives `auto-<16 hex>` from the key
    /// and the text (ADR-0039 d6). A new version takes its target's.
    #[arg(long, conflicts_with = "replaces")]
    claim_key: Option<String>,
    /// memory | derived_fact | steering | term | term_mapping |
    /// vocab_release; omitted, memory, or with --replaces the target's.
    #[arg(long)]
    kind: Option<String>,
    /// Lineage artifact ids; repeatable. Set, a submission is journalled
    /// learn (ADR-0039 d3).
    #[arg(long = "derives-from")]
    derives_from: Vec<String>,
    /// The workarea, by id or name; omitted, the key's default workarea, or
    /// with --replaces the target's, which alone it may name.
    #[arg(long)]
    workarea: Option<String>,
    /// The promoted tip this write replaces, by artifact id: a new version
    /// of its claim, held like any write.
    #[arg(long)]
    replaces: Option<String>,
    /// Propose retiring the --replaces claim; the text says why.
    #[arg(long, requires = "replaces")]
    retirement: bool,
    /// Why the claim changes, journalled with the new version.
    #[arg(long, requires = "replaces")]
    reason: Option<String>,
    /// The write's Idempotency-Key, so a re-run naming it replays the first
    /// answer; omitted, a fresh one per run.
    #[arg(long)]
    idempotency_key: Option<String>,
}

#[derive(Args)]
struct ReadArgs {
    /// Fetch this one version, in any state the key may read.
    #[arg(value_name = "ARTIFACT_ID")]
    artifact_id: Option<String>,
    /// Recall: ranked retrieval over promoted rows, steering first.
    #[arg(long, conflicts_with = "artifact_id")]
    query: Option<String>,
    /// How many hits the recall returns, 1 to 100; omitted, 8.
    #[arg(long, requires = "query")]
    k: Option<u32>,
    /// Recall only these kinds, memory or derived_fact; repeatable.
    #[arg(long, requires = "query")]
    kinds: Vec<String>,
    /// Let the recall return this key's own held rows beside promoted hits.
    #[arg(long, requires = "query")]
    include_unreviewed: bool,
    /// The subtree read, by id or name; omitted, the key's default
    /// workarea, or for the own listing every workarea the key wrote in.
    #[arg(long, conflicts_with = "artifact_id")]
    workarea: Option<String>,
    /// List this key's own rows in every state (the listing's default).
    #[arg(long, conflicts_with_all = ["artifact_id", "query", "all"])]
    own: bool,
    /// List the promoted tips of the workarea's subtree instead.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    all: bool,
    /// List only rows in this state: held, promoted, demoted, rejected or
    /// erased.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    state: Option<String>,
    /// List only rows of this kind.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    kind: Option<String>,
    /// List versions written at or after this RFC 3339 instant.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    since: Option<String>,
    /// List versions written before this RFC 3339 instant.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    until: Option<String>,
    /// The listing's page size.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    limit: Option<u32>,
    /// The next_cursor of the listing's previous page.
    #[arg(long, conflicts_with_all = ["artifact_id", "query"])]
    cursor: Option<String>,
}

#[derive(Args)]
struct EventsArgs {
    /// Resume after this seq: pass back the previous page's next_after_seq.
    #[arg(long)]
    after_seq: Option<u64>,
    /// The subtree tailed, by id or name; omitted, the key's default
    /// workarea.
    #[arg(long)]
    workarea: Option<String>,
    /// How many journal events one call scans.
    #[arg(long)]
    limit: Option<u32>,
}

#[derive(Subcommand)]
enum ReviewCommand {
    /// Serve one held item (claims it for this key).
    Next,
    /// Verdict: promote. Omitted, the serve is the one item this key has
    /// claimed (ADR-0039 d10).
    Approve {
        /// The serve_id `oml review next` printed.
        serve_id: Option<String>,
        /// A note journalled with the verdict.
        #[arg(long)]
        note: Option<String>,
    },
    /// Verdict: reject, with the reason. Omitted, the serve is the one
    /// item this key has claimed (ADR-0039 d10).
    Reject {
        /// The serve_id `oml review next` printed.
        serve_id: Option<String>,
        /// Why the item is rejected; required.
        #[arg(long)]
        reason: String,
    },
    /// The v0.3.0 verdict, hidden until v0.5.0 (ADR-0039 d12).
    #[command(hide = true)]
    Verdict(VerdictArgs),
}

/// `oml submit`, read as `oml write` (ADR-0039 d12).
#[derive(Args)]
struct SubmitArgs {
    #[arg(long)]
    claim: String,
    #[arg(long)]
    claim_key: Option<String>,
    #[arg(long)]
    kind: Option<String>,
    /// v0.3.0's explicit verb, which the HTTP API still honours.
    #[arg(long)]
    verb: Option<String>,
    #[arg(long = "derives-from")]
    derives_from: Vec<String>,
    #[arg(long)]
    workarea: Option<String>,
}

/// `oml review verdict`, read as `oml review approve` or `reject` with the
/// serve the v0.3.0 verb derived from the memory (ADR-0039 d12).
#[derive(Args)]
struct VerdictArgs {
    #[arg(long)]
    memory: Uuid,
    #[arg(long, conflicts_with = "reject")]
    approve: bool,
    #[arg(long)]
    reject: bool,
    #[arg(long)]
    reason: Option<String>,
}

/// `oml recall`, read as `oml read --query` (ADR-0039 d12).
#[derive(Args)]
struct RecallArgs {
    #[arg(long)]
    query: String,
    #[arg(long)]
    k: Option<u32>,
    #[arg(long)]
    workarea: Option<String>,
}

/// `oml supersede`, read as `oml write --replaces` (ADR-0039 d12).
#[derive(Args)]
struct SupersedeArgs {
    #[arg(long)]
    target: Uuid,
    #[arg(long)]
    claim: String,
}

#[derive(Subcommand)]
enum AuditCommand {
    /// Verify the chain per chain-format.md from the chain export; refused
    /// while the server serves none (the events tail is a filtered feed).
    VerifyChain(VerifyChainArgs),
}

#[derive(Args)]
struct VerifyChainArgs {
    /// Run the full-history verification from genesis.
    #[arg(long)]
    genesis: bool,
    /// A deployment signing public key: 64 hex chars (the 32 raw ed25519
    /// bytes). Repeatable — the authenticated key set is the verifier's
    /// input, confirmed out of band (chain-format.md).
    #[arg(long = "signing-key", env = "OML_CLI_SIGNING_KEY")]
    signing_keys: Vec<String>,
    /// Tenant, by id or name (defaults to the key's own, read through
    /// GET /v2/whoami).
    #[arg(long)]
    tenant: Option<String>,
}

/// `<workarea>:<role>`, the workarea by id or name, sent as typed: the
/// server classifies and resolves it, the CLI neither (cli/reference.md
/// conventions; ADR-0026 d4). Neither an id nor a name holds a `:`, so the
/// first one splits the two.
fn parse_grant(s: &str) -> Result<(String, String), String> {
    let (node, role) = s
        .split_once(':')
        .ok_or_else(|| format!("{s:?} is not <workarea>:<role>"))?;
    Ok((node.to_owned(), role.to_owned()))
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
    // The hook reads neither flag, so it names neither (ADR-0029 d4).
    if !matches!(cli.command, Command::Hook { .. }) {
        for (old, new, used) in [
            ("--server", "--url", cli.server.is_some()),
            ("--key", "--api-key", cli.key.is_some()),
        ] {
            if used {
                eprintln!("warning: {old} is renamed {new}; v0.5.0 removes {old} (ADR-0039 d12)");
            }
        }
    }
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
                "`oml tenants create {name}` needs the operator tier, which the PoC does not \
                 compose: the standalone deployment hosts exactly the tenant created by \
                 `oml bootstrap claim` (ADR-0023 d16; cli/reference.md `oml tenants`)"
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
            if let Some(p) = &args.principal {
                body["principal"] = json!(p);
            }
            if let Some(l) = &args.label {
                body["label"] = json!(l);
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
                if let Some(grants) = response["grants"].as_array() {
                    for g in grants {
                        println!(
                            "    grant  : {} : {}",
                            field(g, "workarea_node_id"),
                            field(g, "role")
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
                println!("    default  : {}", field(&response, "default_workarea_id"));
                for g in response["grants"].as_array().into_iter().flatten() {
                    println!(
                        "    grant    : {} : {}",
                        field(g, "workarea_node_id"),
                        field(g, "role")
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

        Command::Workareas {
            command: WorkareasCommand::Tree { root },
        } => run_workareas_tree(cli, &stored, root),

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

        Command::Write(args) => run_write(cli, &stored, &Write::of(args)),
        Command::Read(args) => run_read(cli, &stored, args),
        Command::Events(args) => run_events(cli, &stored, args),
        Command::Review { command } => run_review(cli, &stored, command),

        // The v0.3.0 verbs run their successors, each naming it on one
        // stderr line, until v0.5.0 removes them (ADR-0039 d12).
        Command::Submit(args) => {
            renamed("oml submit", "oml write");
            run_write(
                cli,
                &stored,
                &Write {
                    claim_key: args.claim_key.as_deref(),
                    kind: args.kind.as_deref(),
                    verb: args.verb.as_deref(),
                    derives_from: &args.derives_from,
                    workarea: args.workarea.as_deref(),
                    ..Write::submission(&args.claim)
                },
            )
        }
        Command::Recall(args) => {
            renamed("oml recall", "oml read --query");
            run_read(
                cli,
                &stored,
                &ReadArgs {
                    artifact_id: None,
                    query: Some(args.query.clone()),
                    k: args.k,
                    kinds: Vec::new(),
                    include_unreviewed: false,
                    workarea: args.workarea.clone(),
                    own: false,
                    all: false,
                    state: None,
                    kind: None,
                    since: None,
                    until: None,
                    limit: None,
                    cursor: None,
                },
            )
        }
        Command::Supersede(args) => {
            renamed("oml supersede", "oml write --replaces");
            let target = args.target.to_string();
            run_write(
                cli,
                &stored,
                &Write {
                    replaces: Some(&target),
                    ..Write::submission(&args.claim)
                },
            )
        }

        Command::Audit {
            command: AuditCommand::VerifyChain(args),
        } => run_verify_chain(cli, &stored, args),

        Command::Login { target } => run_login(cli, target),
        Command::Doctor => run_doctor(cli, &stored),
        Command::Hook { .. } => unreachable!("the hook ran before the credentials were loaded"),
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

fn write_opts() -> CallOpts {
    CallOpts {
        auth: true,
        idempotency: true,
    }
}

fn field<'a>(v: &'a Value, name: &str) -> &'a str {
    v[name].as_str().unwrap_or("—")
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
        .map(|(node, role)| json!({ "workarea": node, "role": role }))
        .collect()
}

// ---------------------------------------------------------------------------
// oml write, oml read, oml events, oml review (ADR-0039 d10)
// ---------------------------------------------------------------------------

/// The one stderr line a v0.3.0 verb prints before it runs its successor
/// (ADR-0039 d12).
fn renamed(old: &str, new: &str) {
    eprintln!("warning: `{old}` is renamed `{new}`; v0.5.0 removes `{old}` (ADR-0039 d12)");
}

/// One write, from `oml write`'s flags or a v0.3.0 verb's.
struct Write<'a> {
    text: &'a str,
    claim_key: Option<&'a str>,
    kind: Option<&'a str>,
    /// v0.3.0's explicit verb: only `oml submit` sends one (ADR-0039 d12).
    verb: Option<&'a str>,
    derives_from: &'a [String],
    workarea: Option<&'a str>,
    replaces: Option<&'a str>,
    retirement: bool,
    reason: Option<&'a str>,
    idempotency_key: Option<&'a str>,
}

impl<'a> Write<'a> {
    fn of(args: &'a WriteArgs) -> Self {
        Self {
            text: &args.text,
            claim_key: args.claim_key.as_deref(),
            kind: args.kind.as_deref(),
            verb: None,
            derives_from: &args.derives_from,
            workarea: args.workarea.as_deref(),
            replaces: args.replaces.as_deref(),
            retirement: args.retirement,
            reason: args.reason.as_deref(),
            idempotency_key: args.idempotency_key.as_deref(),
        }
    }

    /// A submission of `text` and nothing else.
    fn submission(text: &'a str) -> Self {
        Self {
            text,
            claim_key: None,
            kind: None,
            verb: None,
            derives_from: &[],
            workarea: None,
            replaces: None,
            retirement: false,
            reason: None,
            idempotency_key: None,
        }
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
    let mut body = json!({ "body": write.text });
    for (name, value) in [
        ("claim_key", write.claim_key),
        ("kind", write.kind),
        ("verb", write.verb),
        ("workarea", write.workarea),
        ("reason", write.reason),
    ] {
        if let Some(value) = value {
            body[name] = json!(value);
        }
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
        ] {
            if let Some(value) = value {
                params.push((name, value.clone()));
            }
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
        ReviewCommand::Approve { serve_id, note } => {
            run_verdict(cli, stored, "approve", serve_id.as_deref(), note.as_deref())
        }
        ReviewCommand::Reject { serve_id, reason } => {
            run_verdict(cli, stored, "reject", serve_id.as_deref(), Some(reason))
        }
        ReviewCommand::Verdict(args) => {
            if !args.approve && !args.reject {
                return Err(CliError::local("a verdict is one of --approve or --reject"));
            }
            // The v0.3.0 verb named the memory, and the PoC serve binding
            // round-trips its id (serve_id = "srv_" + simple uuid).
            let serve_id = format!("srv_{}", args.memory.simple());
            let verdict = if args.approve { "approve" } else { "reject" };
            renamed("oml review verdict", &format!("oml review {verdict}"));
            run_verdict(
                cli,
                stored,
                verdict,
                Some(&serve_id),
                args.reason.as_deref(),
            )
        }
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
            "refused: --out with --connect-code, because a connector key has no value to write \
             and its code is for a person to read and type within minutes; nothing was sent \
             (ADR-0031 d7)",
        ));
    }
    if cli.json && args.out.is_some() {
        return Err(CliError::local(
            "refused: --out with --json, because the 201 body --json prints carries the key \
             value; nothing was sent (ADR-0028 d1)",
        ));
    }
    let client = resolve(cli, stored)?;
    let key_file = match &args.out {
        Some(path) => Some(KeyFile::create(path).map_err(CliError::Local)?),
        None => None,
    };
    let grants = with_reader_beside_writers(
        &[(args.workarea.clone(), "writer".to_owned())],
        args.write_only,
    );
    let mut body = json!({
        "role": "writer",
        "name": args.key_name,
        "held_by": args.held_by,
        "principal": args.principal,
        "default_workarea": args.workarea,
        "grants": grants_json(&grants),
    });
    if let Some(crew) = &args.crew {
        body["crew"] = json!(crew);
    }
    if args.connect_code {
        body["connect_code"] = json!(true);
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
                    "{why}. A connector key named {name} for principal {principal} may have been \
                     issued. If it was, its connect code is lost: `oml keys show {name}` answers \
                     whether it exists, and `oml keys connect-code {name}` mints a new code \
                     (ADR-0031 d7)",
                    name = args.key_name,
                    principal = args.principal,
                )),
                CliError::Unanswered(why) => CliError::Unanswered(format!(
                    "{why}. A key named {name} for principal {principal} may have been issued. If \
                     it was, its value is lost, since the server keeps only its hash and the CLI \
                     kept none of the answer: `oml keys show {name}` answers whether it exists. \
                     Issue a new key under a new name, since no name is released, and revoke \
                     this one; until key revocation is built, `oml deny set {name} universe` \
                     keeps it from every workarea (ADR-0028 d1; ADR-0026 d10)",
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
            "key {key_name} ({key_id}) was issued, but {why}: its value is lost and the key is \
             live. Revoke key {key_id}; until key revocation is built, `oml deny set {key_id} \
             universe` keeps it from every workarea (ADR-0028 d1)"
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
                Some(configure_line(client.server(), &shown)),
            )
        }
    };
    println!("export OML_URL={}", sh_quote(client.server()));
    println!("{key_line}");
    if let Some(configure) = configure {
        println!("{configure}");
    }

    let granted: Vec<String> = response["grants"]
        .as_array()
        .map(|grants| {
            grants
                .iter()
                .map(|g| format!("{}:{}", field(g, "workarea_node_id"), field(g, "role")))
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
            "logged in to {} as key {} ({}): role {}, tenant {}, default workarea {}",
            client.server(),
            field(&me, "name"),
            field(&me, "key_id"),
            field(&me, "role"),
            field(&me["tenant"], "name"),
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
// oml doctor
// ---------------------------------------------------------------------------

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
    if let Some(url) = source.as_ref().and_then(|s| s.url) {
        let keyless = Client::new(url.clone(), None);
        health = Some(keyless.probe("/healthz")?);
        ready = Some(keyless.probe("/readyz")?);
        if let Some(key) = source.as_ref().and_then(|s| s.key) {
            let client = Client::new(url.clone(), Some(key.clone()));
            me = Some(client.whoami());
            head_seq = client
                .get("/v2/events?after_seq=0&limit=1")
                .ok()
                .and_then(|(_, v)| v["head_seq"].as_u64());
        }
    }
    let probes_ok = matches!((&health, &ready), (Some((200, _)), Some((200, _))));
    let whoami_ok = !matches!(me, Some(Err(_)));
    let outcome = if !probes_ok || !whoami_ok {
        "fail"
    } else if me.is_none() {
        "warn"
    } else {
        "ok"
    };
    let whoami = match &me {
        Some(Ok(v)) => json!({
            "role": v["role"],
            "tenant": v["tenant"]["name"],
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
            "client_version": env!("CARGO_PKG_VERSION"),
            // The PoC API exposes no server version endpoint.
            "server_version": Value::Null,
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
        match head_seq {
            Some(seq) => println!("chain head  : seq {seq}"),
            None => println!("chain head  : unavailable (needs a key with events-tail access)"),
        }
        println!("client      : oml {}", env!("CARGO_PKG_VERSION"));
        println!("server      : version not exposed by the PoC API");
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
