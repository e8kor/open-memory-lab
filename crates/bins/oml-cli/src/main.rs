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

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgGroup, Args, Parser, Subcommand};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use oml_cli::client::{CallOpts, CliError, Client, encode, segment};
use oml_cli::creds::{self, Credentials};
use oml_cli::hook;
use oml_cli::issue::{KeyFile, sh_quote, with_reader_beside_writers};
use oml_cli::verify::parse_verifying_key;

#[derive(Parser)]
#[command(
    name = "oml",
    version,
    about = "Open Memory Lab administration — a lab client of the same HTTP API every agent speaks (ADR-0015)."
)]
struct Cli {
    /// Server URL (falls back to the stored credentials).
    #[arg(long, global = true, env = "OML_CLI_SERVER")]
    server: Option<String>,
    /// API key value (falls back to OML_CLI_KEY, then the stored credentials).
    #[arg(long, global = true, env = "OML_CLI_KEY", hide_env_values = true)]
    key: Option<String>,
    /// Emit the server's typed response as one JSON document on stdout.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
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
    /// Submit a claim (writer convenience for the demo).
    Submit(SubmitArgs),
    /// The review ceremony.
    Review {
        #[command(subcommand)]
        command: ReviewCommand,
    },
    /// Recall promoted memories.
    Recall(RecallArgs),
    /// Supersede a promoted claim with a successor (author only).
    Supersede(SupersedeArgs),
    /// Evidence verbs.
    Audit {
        #[command(subcommand)]
        command: AuditCommand,
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
    /// Print the SessionStart context from OML_URL and OML_API_KEY alone —
    /// steering, this key's recent submissions, the matching memories — or
    /// nothing; always exits 0 (ADR-0029).
    SessionStart,
}

#[derive(Subcommand)]
enum BootstrapCommand {
    /// Exchange the single-use enrollment token for the first admin key and
    /// store the credentials locally (0600).
    Claim {
        /// The name of the key the claim mints, unique in its namespace and
        /// never released (ADR-0026 d9, d12).
        key_name: String,
        /// The enrollment token the lead printed at first boot.
        #[arg(long)]
        token: String,
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
    /// print the OML_URL and OML_API_KEY export lines it reads (ADR-0028 d1).
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
struct SubmitArgs {
    /// The claim body text.
    #[arg(long)]
    claim: String,
    /// Claim key; defaults to cli-<sha256 prefix of the body>.
    #[arg(long)]
    claim_key: Option<String>,
    #[arg(long, default_value = "memory")]
    kind: String,
    /// remember | learn (learn requires --derives-from).
    #[arg(long, default_value = "remember")]
    verb: String,
    /// Lineage artifact ids; repeatable.
    #[arg(long = "derives-from")]
    derives_from: Vec<Uuid>,
    /// X-OML-Workarea override (workarea id or name).
    #[arg(long)]
    workarea: Option<String>,
}

#[derive(Subcommand)]
enum ReviewCommand {
    /// Serve one held item (claims it for this key).
    Next,
    /// Record a verdict against a served item.
    Verdict(VerdictArgs),
}

#[derive(Args)]
struct VerdictArgs {
    /// The served memory's artifact id.
    #[arg(long)]
    memory: Uuid,
    #[arg(long, conflicts_with = "reject")]
    approve: bool,
    #[arg(long)]
    reject: bool,
    /// Required to reject.
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Args)]
struct RecallArgs {
    #[arg(long)]
    query: String,
    #[arg(long, default_value_t = 8)]
    k: usize,
    /// X-OML-Workarea override (workarea id or name).
    #[arg(long)]
    workarea: Option<String>,
}

#[derive(Args)]
struct SupersedeArgs {
    /// The promoted artifact to supersede (must be the current tip).
    #[arg(long)]
    target: Uuid,
    /// The successor claim body (the successor keeps the target's claim_key).
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
    /// Tenant, by id or name (defaults to the stored credentials' tenant).
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

/// defaults < credentials file < flags/environment (clap already applied the
/// env vars), mirroring the server's own precedence law.
fn resolve(cli: &Cli, stored: &Credentials) -> Result<Client, CliError> {
    let server = cli
        .server
        .clone()
        .or_else(|| stored.server.clone())
        .ok_or_else(|| {
            CliError::local(
                "no server: pass --server, set OML_CLI_SERVER, or run `oml bootstrap claim`",
            )
        })?;
    let key = cli.key.clone().or_else(|| stored.key.clone());
    Ok(Client::new(server, key))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
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
    // The hook reads OML_URL and OML_API_KEY alone, so it runs before the
    // stored admin credentials are loaded, and ignores --server and --key
    // (ADR-0029 d4).
    if let Command::Hook {
        command: HookCommand::SessionStart,
    } = &cli.command
    {
        return Ok(run_session_start());
    }
    let stored = creds::load();
    match &cli.command {
        Command::Bootstrap {
            command:
                BootstrapCommand::Claim {
                    key_name,
                    token,
                    tenant_name,
                    principal,
                },
        } => {
            let client = resolve(cli, &stored)?;
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
                    workarea: None,
                },
            )?;
            let path = creds::store(&Credentials {
                server: Some(client.server().to_owned()),
                key: response["value"].as_str().map(ToOwned::to_owned),
                key_id: response["key_id"].as_str().map(ToOwned::to_owned),
                tenant_id: response["tenant_id"].as_str().map(ToOwned::to_owned),
                root_workarea_id: response["root_workarea_id"].as_str().map(ToOwned::to_owned),
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
                body["default_workarea_id"] = json!(wa);
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
            let mut body = json!({ "workarea_node_id": subtree });
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
            let mut body = json!({ "parent_id": parent, "name": name });
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

        Command::Submit(args) => {
            let client = resolve(cli, &stored)?;
            let claim_key = args.claim_key.clone().unwrap_or_else(|| {
                let digest = hex::encode(Sha256::digest(args.claim.as_bytes()));
                format!("cli-{}", &digest[..12])
            });
            let body = json!({
                "verb": args.verb,
                "claim_key": claim_key,
                "kind": args.kind,
                "body": args.claim,
                "derives_from": args.derives_from,
            });
            let opts = CallOpts {
                auth: true,
                idempotency: true,
                workarea: args.workarea.clone(),
            };
            let (_, response) = client.post("/v1/memories", &body, &opts)?;
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
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Review {
            command: ReviewCommand::Next,
        } => {
            let client = resolve(cli, &stored)?;
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
                    field(item, "body").replace('\n', "\n            | ")
                );
                println!();
                println!(
                    "  serve_id: {}  (expires {})",
                    field(&response, "serve_id"),
                    field(&response, "expires_at")
                );
                println!(
                    "  verdict with: oml review verdict --memory {} --approve|--reject --reason …",
                    field(item, "artifact_id")
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Review {
            command: ReviewCommand::Verdict(args),
        } => {
            if !args.approve && !args.reject {
                return Err(CliError::local("a verdict is one of --approve or --reject"));
            }
            let client = resolve(cli, &stored)?;
            // The PoC serve binding round-trips the memory id
            // (phase-4 transport: serve_id = "srv_" + simple uuid).
            let serve_id = format!("srv_{}", args.memory.simple());
            let verdict = if args.approve { "approve" } else { "reject" };
            let mut body = json!({ "serve_id": serve_id, "verdict": verdict });
            if let Some(reason) = &args.reason {
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

        Command::Recall(args) => {
            let client = resolve(cli, &stored)?;
            let opts = CallOpts {
                auth: true,
                idempotency: false, // read-shaped POST (http-api.md idempotency)
                workarea: args.workarea.clone(),
            };
            let (_, response) = client.post(
                "/v1/memories/recall",
                &json!({ "query": args.query, "k": args.k, "include_bodies": true }),
                &opts,
            )?;
            if cli.json {
                print_json(&response);
            } else {
                let hits = response["hits"].as_array().cloned().unwrap_or_default();
                println!(
                    "{} hit(s) — query digest {}",
                    hits.len(),
                    field(&response, "query_digest")
                );
                for hit in &hits {
                    let memory = &hit["memory"];
                    println!();
                    println!(
                        "  {}  {}  ({})",
                        field(memory, "artifact_id"),
                        field(memory, "claim_key"),
                        field(memory, "state")
                    );
                    println!("    | {}", field(memory, "body").replace('\n', "\n    | "));
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Supersede(args) => {
            let client = resolve(cli, &stored)?;
            let (_, response) = client.post(
                &format!("/v1/memories/{}/supersede", args.target),
                &json!({ "body": args.claim }),
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "accepted — successor is {} (journal seq {})",
                    field(&response, "state"),
                    response["journal_seq"]
                );
                println!("  artifact  : {}", field(&response, "artifact_id"));
                println!("  claim_key : {}", field(&response, "claim_key"));
                println!("  digest    : {}", field(&response, "plaintext_digest"));
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Audit {
            command: AuditCommand::VerifyChain(args),
        } => run_verify_chain(cli, &stored, args),

        Command::Doctor => run_doctor(cli, &stored),
        Command::Hook { .. } => unreachable!("the hook ran before the credentials were loaded"),
    }
}

/// `oml hook session-start`: the hook document on stdout, or nothing, and at
/// most one line on stderr; exit 0 whatever happened, so a session always
/// starts (ADR-0029 d5). The hook runs on its own thread under
/// `hook::DEADLINE`: returning from `main` ends the process even while that
/// thread is stuck where no timeout reaches, as in a DNS lookup.
fn run_session_start() -> ExitCode {
    use std::io::Write as _;
    let (done, outcome) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let var = |name: &str| std::env::var(name).ok();
        let _ = done.send(hook::session_start(&var, &mut std::io::stdin().lock()));
    });
    let reason = match outcome.recv_timeout(hook::DEADLINE) {
        Ok(Ok(document)) => {
            // A closed stream is not a reason to exit non-zero: writes are
            // not unwrapped, as println! would.
            let _ = writeln!(std::io::stdout(), "{document}");
            None
        }
        Ok(Err(reason)) => reason,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Some(format!(
            "no answer within {} seconds, so no context was loaded",
            hook::DEADLINE.as_secs()
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Some("the hook stopped before it finished, so no context was loaded".to_owned())
        }
    };
    if let Some(reason) = reason {
        let _ = writeln!(std::io::stderr(), "oml hook session-start: {reason}");
    }
    ExitCode::SUCCESS
}

fn write_opts() -> CallOpts {
    CallOpts {
        auth: true,
        idempotency: true,
        workarea: None,
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
        .map(|(node, role)| json!({ "workarea_node_id": node, "role": role }))
        .collect()
}

// ---------------------------------------------------------------------------
// oml keys issue-agent (ADR-0028 d1)
// ---------------------------------------------------------------------------

/// One `POST /v2/keys` for a writer key named `<key-name>`, with writer and
/// reader grants on the workarea, that workarea as its default and the
/// `--crew` when one is given, each reference as the operator spelled it,
/// then exactly two lines on stdout: `export OML_URL=…` and `export
/// OML_API_KEY=…`, the second reading the value back from the `--out` file
/// when one is named. Every other line goes to stderr, and no key value
/// reaches either stream once `--out` is given (cli/reference.md `oml keys
/// issue-agent`; ADR-0028 d1; ADR-0026 d12).
fn run_issue_agent(
    cli: &Cli,
    stored: &Credentials,
    args: &IssueAgentArgs,
) -> Result<ExitCode, CliError> {
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
        "default_workarea_id": args.workarea,
        "grants": grants_json(&grants),
    });
    if let Some(crew) = &args.crew {
        body["crew"] = json!(crew);
    }
    let response = match client.post("/v2/keys", &body, &write_opts()) {
        Ok((_, response)) => response,
        Err(failure) => {
            // The empty file holds nothing either way, so it goes.
            if let Some(Err(e)) = key_file.map(KeyFile::discard) {
                eprintln!("note: {e}");
            }
            return Err(match failure {
                // Sent, and no usable answer: a key may exist whose value
                // nothing kept. Only a refusal, or a failure before sending,
                // means no key exists (ADR-0028 d1).
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
    let key_line = match key_file {
        None => format!("export OML_API_KEY={}", sh_quote(value)),
        Some(file) => {
            let shown = file.shown().to_owned();
            file.write_value(value)
                .map_err(|e| lost(&format!("writing {shown} failed ({e})")))?;
            format!("export OML_API_KEY=\"$(cat {})\"", sh_quote(&shown))
        }
    };
    println!("export OML_URL={}", sh_quote(client.server()));
    println!("{key_line}");

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
            "the value is only in the key file (0600); delete it once the key is in the \
             environment that needs it"
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
    resolve(cli, stored)?;
    let tenant = args
        .tenant
        .clone()
        .or_else(|| stored.tenant_id.clone())
        .ok_or_else(|| {
            CliError::local(
                "verification reconstructs the signed bytes, which include the tenant id: pass \
                 --tenant or run `oml bootstrap claim` first (chain-format.md signed fields)",
            )
        })?;
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
    let mode = if args.genesis { " from genesis" } else { "" };
    Err(CliError::local(format!(
        "live verification of tenant {tenant}{mode} reads the chain export (kind: chain), which \
         this server does not serve yet; the events tail is a filtered feed, not a verification \
         surface (http-api.md events tail)"
    )))
}

// ---------------------------------------------------------------------------
// oml doctor
// ---------------------------------------------------------------------------

fn run_doctor(cli: &Cli, stored: &Credentials) -> Result<ExitCode, CliError> {
    let client = resolve(cli, stored)?;
    let (health_status, health) = client.probe("/healthz")?;
    let (ready_status, ready) = client.probe("/readyz")?;
    let healthy = health_status == 200 && ready_status == 200;

    // Chain head via the events tail, when a key is in context.
    let head_seq: Option<u64> = client
        .get("/v2/events?after_seq=0&limit=1")
        .ok()
        .and_then(|(_, v)| v["head_seq"].as_u64());

    if cli.json {
        print_json(&json!({
            "healthz": { "status": health_status, "body": health },
            "readyz": { "status": ready_status, "body": ready },
            "chain_head_seq": head_seq,
            "client_version": env!("CARGO_PKG_VERSION"),
            // The PoC API exposes no server version endpoint.
            "server_version": Value::Null,
            "verdict": if healthy { "ok" } else { "fail" },
        }));
    } else {
        println!(
            "healthz     : {} ({})",
            verdict(health_status == 200),
            field(&health, "status")
        );
        println!("readyz      : {}", verdict(ready_status == 200));
        if let Some(checks) = ready["checks"].as_object() {
            for (name, state) in checks {
                println!("  {name:<10}: {}", state.as_str().unwrap_or("—"));
            }
        }
        match head_seq {
            Some(seq) => println!("chain head  : seq {seq}"),
            None => println!("chain head  : unavailable (needs a key with events-tail access)"),
        }
        println!("client      : oml {}", env!("CARGO_PKG_VERSION"));
        println!("server      : version not exposed by the PoC API");
        println!("verdict     : {}", verdict(healthy));
    }
    Ok(if healthy {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn verdict(ok: bool) -> &'static str {
    if ok { "ok" } else { "fail" }
}
