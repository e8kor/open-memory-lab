//! `oml demo seed`: a demonstration workarea filled through the public API
//! alone (ADR-0067 d4). It creates the workarea, three short-lived identities
//! (one author and two reviewers, so the reviewer is never the author), and
//! writes a team's memories: most approved, some superseded by a newer approved
//! version, some left held. Every identity is fenced from the whole tree when
//! the seed ends, so no key outlives it and none is printed.

use serde_json::{Value, json};

use crate::client::{CallOpts, CliError, Client, segment};

/// What the seed does with one claim.
pub enum Fate {
    /// Written, then approved by a reviewer.
    Approved,
    /// Written, approved, then replaced by this text, which is approved too:
    /// the first text is history and recall serves the second.
    Superseded(&'static str),
    /// Written and left for a reviewer.
    Held,
}

/// One memory of the demonstration team, a fictional freight company.
pub struct Memory {
    pub text: &'static str,
    pub fate: Fate,
}

const fn approved(text: &'static str) -> Memory {
    Memory {
        text,
        fate: Fate::Approved,
    }
}

const fn superseded(text: &'static str, newer: &'static str) -> Memory {
    Memory {
        text,
        fate: Fate::Superseded(newer),
    }
}

const fn held(text: &'static str) -> Memory {
    Memory {
        text,
        fate: Fate::Held,
    }
}

/// Thirty claims: twenty approved, four approved and then superseded, six held.
pub const MEMORIES: [Memory; 30] = [
    approved(
        "Production deploys run from the main branch only, through the release pipeline; a deploy from a feature branch is rejected by the pipeline's branch check.",
    ),
    approved(
        "The orders service reads its configuration from environment variables at start and does not reload them; a changed value needs a restart.",
    ),
    approved(
        "Database migrations on the orders database are forward-only; a mistaken migration is corrected by the next numbered file, never by editing the applied one.",
    ),
    approved(
        "The payments sandbox rate-limits to 50 requests per minute per account; test runs that exceed it fail with HTTP 429 and pass on a retry after a minute.",
    ),
    approved(
        "Customer-facing timestamps are stored in UTC and rendered in the viewer's time zone by the web client; the API never returns local times.",
    ),
    approved(
        "The nightly export job finishes before 03:00 UTC on a normal night; if it is still running at 04:00 the on-call engineer is paged.",
    ),
    approved(
        "A pull request needs one approving review from someone other than its author before it can merge.",
    ),
    approved(
        "The staging environment is rebuilt from production's schema every Sunday at 02:00 UTC, so data created there by hand does not survive the weekend.",
    ),
    approved(
        "Feature flags are named after the feature, never the ticket, and are removed within two releases of full rollout.",
    ),
    approved(
        "The search index lags the orders database by up to 30 seconds, so a read immediately after a write may not find the new order.",
    ),
    approved(
        "Image uploads over 8 MB are rejected at the edge with HTTP 413; the web client resizes an image before it uploads.",
    ),
    approved(
        "The on-call handover happens on Monday at 10:00 UTC, and the outgoing engineer writes a summary in the operations channel.",
    ),
    approved(
        "Build agents cache dependency downloads for seven days; a build that fails in the pipeline and passes locally is first retried with the cache cleared.",
    ),
    approved(
        "Service-to-service calls carry a request id header and every service logs it, so one request can be followed across services.",
    ),
    approved(
        "Invoices are numbered sequentially per calendar year and a number is never reused, even when the invoice is voided.",
    ),
    approved(
        "The warehouse API returns stock counts as whole units and weights in grams, never kilograms.",
    ),
    approved("Password reset links expire after 30 minutes and work once."),
    approved(
        "Load tests run against staging on Tuesdays between 13:00 and 15:00 UTC, when no release is scheduled.",
    ),
    approved(
        "The mobile app supports the two most recent major versions of iOS and Android; older versions get a prompt to update.",
    ),
    approved(
        "Incident reviews are blameless and are published within five working days, with one owner named for each follow-up action.",
    ),
    superseded(
        "The orders service connection pool is capped at 20 connections per instance.",
        "The orders service connection pool is capped at 40 connections per instance since the database upgrade; the old cap of 20 caused queueing during checkout peaks.",
    ),
    superseded(
        "The release freeze starts on the Thursday before a public holiday.",
        "The release freeze starts on the Wednesday before a public holiday and lifts on the first working day after it.",
    ),
    superseded(
        "Application logs are kept for 14 days.",
        "Application logs are kept for 30 days; audit logs are kept for one year.",
    ),
    superseded(
        "Support tickets are triaged within one business day.",
        "Support tickets are triaged within four business hours on weekdays, and urgent tickets within one hour.",
    ),
    held(
        "The checkout page could show delivery estimates in business days rather than calendar days.",
    ),
    held(
        "The shipping provider's API returned duplicate tracking numbers once in October; storing orders should deduplicate on the tracking number.",
    ),
    held(
        "The new tax rules for cross-border orders appear to apply from 1 January, pending confirmation from finance.",
    ),
    held(
        "Dependency updates for the web client could move to a weekly schedule instead of one per release.",
    ),
    held(
        "The reporting database replica occasionally lags by several minutes during the month-end close.",
    ),
    held(
        "Customer exports might need a row limit; one export of two million rows timed out last week.",
    ),
];

/// What a finished seed reports.
#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub workarea: String,
    pub approved: usize,
    pub superseded: usize,
    pub held: usize,
    /// The key names fenced at the end: author, first reviewer, second reviewer.
    pub identities: [String; 3],
}

/// The three names a seed of `workarea` gives its identities.
#[must_use]
pub fn identity_names(workarea: &str) -> [String; 3] {
    [
        format!("{workarea}-author"),
        format!("{workarea}-reviewer-a"),
        format!("{workarea}-reviewer-b"),
    ]
}

fn opts() -> CallOpts {
    CallOpts {
        auth: true,
        idempotency: true,
    }
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, CliError> {
    value[field]
        .as_str()
        .ok_or_else(|| CliError::local(format!("the server's answer carried no {field}: {value}")))
}

/// Issues one key under `admin` and returns its value, held in memory only.
fn issue(
    admin: &Client,
    name: &str,
    role: &str,
    held_by: &str,
    workarea: &str,
    grant_roles: &[&str],
) -> Result<String, CliError> {
    let grants: Vec<Value> = grant_roles
        .iter()
        .map(|r| json!({ "workarea": workarea, "role": r }))
        .collect();
    let body = json!({
        "role": role,
        "name": name,
        "held_by": held_by,
        "principal": format!("demo/{name}"),
        "default_workarea": workarea,
        "grants": grants,
    });
    let (_, response) = admin.post("/v2/keys", &body, &opts())?;
    Ok(text(&response, "value")?.to_owned())
}

/// Claims the next held item under `reviewer`, checks it is `artifact`, and
/// approves it; the item's own verdict is the only one this key gives.
fn approve(reviewer: &Client, artifact: &str) -> Result<(), CliError> {
    let (status, served) = reviewer.post("/v2/review/next", &json!({}), &opts())?;
    if status == 204 {
        return Err(CliError::local(format!(
            "the review queue was empty where {artifact} should wait"
        )));
    }
    let item = text(&served["item"], "artifact_id")?;
    if item != artifact {
        return Err(CliError::local(format!(
            "the review queue served {item} where {artifact} was written; the workarea is not new"
        )));
    }
    let body = json!({ "verdict": "approve", "serve_id": text(&served, "serve_id")? });
    let (_, verdict) = reviewer.post("/v2/review/verdicts", &body, &opts())?;
    if verdict["state"] != "promoted" {
        return Err(CliError::local(format!(
            "the approval of {artifact} did not promote it: {verdict}"
        )));
    }
    Ok(())
}

/// Seeds `workarea` under `parent` through `admin`, which must be an admin
/// key. Nothing is retried and nothing is undone on failure: the identities
/// it issued are fenced even then, so a failed seed leaves no live key.
///
/// # Errors
///
/// The refusal or failure of the first call that did not succeed.
pub fn seed(admin: &Client, workarea: &str, parent: &str) -> Result<Report, CliError> {
    let (_, created) = admin.post(
        "/v2/workareas",
        &json!({ "parent": parent, "name": workarea }),
        &opts(),
    )?;
    let identities = identity_names(workarea);
    let mut issued = Vec::new();
    let result = (|| {
        let author = issue(
            admin,
            &identities[0],
            "writer",
            "agent",
            workarea,
            &["writer", "reader"],
        )?;
        issued.push(identities[0].clone());
        let first = issue(
            admin,
            &identities[1],
            "dreamer",
            "human",
            workarea,
            &["dreamer"],
        )?;
        issued.push(identities[1].clone());
        let second = issue(
            admin,
            &identities[2],
            "dreamer",
            "human",
            workarea,
            &["dreamer"],
        )?;
        issued.push(identities[2].clone());
        let server = admin.server().to_owned();
        let writer = Client::new(server.clone(), Some(author));
        let reviewers = [
            Client::new(server.clone(), Some(first)),
            Client::new(server, Some(second)),
        ];
        run(&writer, &reviewers, workarea)
    })();
    for name in &issued {
        let path = format!("/v2/keys/{}/denials", segment(name)?);
        let fenced = admin.post(&path, &json!({ "workarea": "universe" }), &opts());
        if result.is_ok() {
            fenced?;
        }
    }
    let (approved, superseded, held) = result?;
    Ok(Report {
        workarea: text(&created, "name").unwrap_or(workarea).to_owned(),
        approved,
        superseded,
        held,
        identities,
    })
}

/// The writes and verdicts; returns (approved, superseded, held) claim counts.
/// Approved claims and replacements alternate between the two reviewers, and
/// the held claims are written last, after every review is done, because a
/// reviewer is served the oldest held item first.
fn run(
    writer: &Client,
    reviewers: &[Client; 2],
    workarea: &str,
) -> Result<(usize, usize, usize), CliError> {
    let mut turn = 0_usize;
    let mut next_reviewer = || {
        turn += 1;
        &reviewers[turn % 2]
    };
    let (mut approved, mut superseded, mut held) = (0, 0, 0);
    let write = |n: usize, path: &str, body: &Value| -> Result<String, CliError> {
        let (_, receipt) = writer.post_keyed(path, body, &format!("demo-seed:{workarea}:{n}"))?;
        Ok(text(&receipt, "artifact_id")?.to_owned())
    };
    for (n, memory) in MEMORIES.iter().enumerate() {
        match memory.fate {
            Fate::Approved => {
                let id = write(n, "/v1/memories", &json!({ "body": memory.text }))?;
                approve(next_reviewer(), &id)?;
                approved += 1;
            }
            Fate::Superseded(newer) => {
                let id = write(n, "/v1/memories", &json!({ "body": memory.text }))?;
                approve(next_reviewer(), &id)?;
                let path = format!("/v1/memories/{}/supersede", segment(&id)?);
                let body = json!({ "body": newer, "expected_version": 1 });
                let successor = write(1000 + n, &path, &body)?;
                approve(next_reviewer(), &successor)?;
                approved += 1;
                superseded += 1;
            }
            Fate::Held => {}
        }
    }
    for (n, memory) in MEMORIES.iter().enumerate() {
        if matches!(memory.fate, Fate::Held) {
            write(n, "/v1/memories", &json!({ "body": memory.text }))?;
            held += 1;
        }
    }
    Ok((approved, superseded, held))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{Fate, MEMORIES, identity_names};

    /// ADR-0067 d4: thirty claims, twenty plain approvals, four supersessions
    /// and six held, no text twice, each a sentence a reviewer could judge.
    #[test]
    fn the_demo_has_thirty_distinct_claims_in_the_stated_states() {
        assert_eq!(MEMORIES.len(), 30);
        let count = |f: fn(&Fate) -> bool| MEMORIES.iter().filter(|m| f(&m.fate)).count();
        assert_eq!(count(|f| matches!(f, Fate::Approved)), 20);
        assert_eq!(count(|f| matches!(f, Fate::Superseded(_))), 4);
        assert_eq!(count(|f| matches!(f, Fate::Held)), 6);
        let mut texts = HashSet::new();
        for memory in &MEMORIES {
            let mut all = vec![memory.text];
            if let Fate::Superseded(newer) = memory.fate {
                all.push(newer);
            }
            for text in all {
                assert!(texts.insert(text), "{text:?} appears twice");
                assert!(text.ends_with('.'), "{text:?} is not a sentence");
                assert!(text.len() < 400, "{text:?} is too long for a claim");
                assert!(!text.contains("[oml:"), "{text:?} carries a scanner marker");
            }
        }
    }

    /// The three identities are distinct names under the workarea, so the
    /// reviewers are never the author.
    #[test]
    fn the_author_and_the_two_reviewers_are_three_names() {
        let names = identity_names("reviewer-demo");
        assert_eq!(
            names,
            [
                "reviewer-demo-author",
                "reviewer-demo-reviewer-a",
                "reviewer-demo-reviewer-b"
            ]
        );
    }
}
