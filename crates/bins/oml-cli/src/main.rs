//! `oml` — every administrative ceremony is a CLI verb riding the lab's own
//! HTTP API (ADR-0015, cli/reference.md). Exit codes per the reference's
//! conventions: 0 success, 1 a typed server refusal (or a failed
//! verification/doctor verdict), 2 local failure. `--json` emits exactly one
//! JSON document on stdout — the server's typed response, unreshaped.

// The argument doc comments are clap's --help text, where a placeholder such
// as <role> must print as written rather than as markdown code.
#![allow(rustdoc::invalid_html_tags)]

use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use oml_cli::client::{CallOpts, CliError, Client};
use oml_cli::creds::{self, Credentials};
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
}

#[derive(Subcommand)]
enum BootstrapCommand {
    /// Exchange the single-use enrollment token for the first admin key and
    /// store the credentials locally (0600).
    Claim {
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
}

#[derive(Args)]
struct KeysIssueArgs {
    /// reader | writer | dreamer | admin
    #[arg(long)]
    role: String,
    /// agent | human | service
    #[arg(long, default_value = "agent")]
    held_by: String,
    /// Workarea node the provider ring targets when a request names none;
    /// some grant must cover it. Omitted, the first grant's node.
    #[arg(long)]
    default_workarea: Option<Uuid>,
    /// Grant as <workarea-node-id>:<role>; repeatable.
    #[arg(long = "grant", value_parser = parse_grant)]
    grants: Vec<(Uuid, String)>,
    /// Principal the key acts as.
    #[arg(long)]
    principal: Option<String>,
    #[arg(long)]
    label: Option<String>,
}

#[derive(Subcommand)]
enum DenyCommand {
    /// Create or extend the key's global deny object over a subtree. Distinct
    /// from a per-grant deny; journalled as key.denied.
    Set {
        /// The key the denial binds.
        key_id: Uuid,
        /// The workarea node id whose subtree the key no longer reaches.
        subtree: Uuid,
        /// Why; the server accepts it and journals no reason.
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand)]
enum WorkareasCommand {
    /// Create a child workarea in development status.
    Create {
        /// Parent workarea node id.
        parent: Uuid,
        name: String,
    },
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
    /// X-OML-Workarea override (workarea node id).
    #[arg(long)]
    workarea: Option<Uuid>,
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
    /// X-OML-Workarea override (workarea node id).
    #[arg(long)]
    workarea: Option<Uuid>,
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
    /// Tenant id (defaults to the stored credentials' tenant).
    #[arg(long)]
    tenant: Option<Uuid>,
}

fn parse_grant(s: &str) -> Result<(Uuid, String), String> {
    let (node, role) = s
        .split_once(':')
        .ok_or_else(|| format!("{s:?} is not <workarea-node-id>:<role>"))?;
    let id = Uuid::parse_str(node).map_err(|_| format!("{node:?} is not a workarea node id"))?;
    Ok((id, role.to_owned()))
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
        Err(CliError::Local(message)) => {
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
    let stored = creds::load();
    match &cli.command {
        Command::Bootstrap {
            command:
                BootstrapCommand::Claim {
                    token,
                    tenant_name,
                    principal,
                },
        } => {
            let client = resolve(cli, &stored)?;
            let mut body = json!({ "enrollment_token": token });
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
            let grants: Vec<Value> = args
                .grants
                .iter()
                .map(|(node, role)| json!({ "workarea_node_id": node, "role": role }))
                .collect();
            let mut body = json!({
                "role": args.role,
                "held_by": args.held_by,
                "grants": grants,
            });
            if let Some(wa) = args.default_workarea {
                body["default_workarea_id"] = json!(wa);
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

        Command::Deny {
            command:
                DenyCommand::Set {
                    key_id,
                    subtree,
                    reason,
                },
        } => {
            let client = resolve(cli, &stored)?;
            let mut body = json!({ "workarea_node_id": subtree });
            if let Some(reason) = reason {
                body["reason"] = json!(reason);
            }
            let (_, response) =
                client.post(&format!("/v2/keys/{key_id}/denials"), &body, &write_opts())?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "denial journalled — key {key_id} no longer reaches {} (epoch {})",
                    field(&response, "workarea_node_id"),
                    response["epoch"]
                );
                println!("  denial id : {}", field(&response, "denial_id"));
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Workareas {
            command: WorkareasCommand::Create { parent, name },
        } => {
            let client = resolve(cli, &stored)?;
            let (_, response) = client.post(
                "/v2/workareas",
                &json!({ "parent_id": parent, "name": name }),
                &write_opts(),
            )?;
            if cli.json {
                print_json(&response);
            } else {
                println!(
                    "created {} {} (status: {})",
                    field(&response, "id"),
                    field(&response, "path"),
                    field(&response, "status")
                );
            }
            Ok(ExitCode::SUCCESS)
        }

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
                workarea: args.workarea.map(|w| w.to_string()),
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
                workarea: args.workarea.map(|w| w.to_string()),
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
    }
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

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
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
        .map(|t| t.to_string())
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
