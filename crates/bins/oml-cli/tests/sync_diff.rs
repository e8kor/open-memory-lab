//! The diff and the run of `oml sync` (ADR-0057 d8–d11, d14, d18–d20, d16
//! step 3–4) against a fake of the HTTP API that keeps versions, review
//! states, source checks, idempotency records and claim edges in memory.

use std::cell::RefCell;
use std::collections::HashMap;

use serde_json::{Value, json};

use oml_cli::client::CliError;
use oml_cli::sync::adapter::Kind;
use oml_cli::sync::api::{EdgeRow, ListQuery, Listed, Me, SyncApi};
use oml_cli::sync::plan::Scope;
use oml_cli::sync::run::{Loaded, Options, Report, execute};

const REPO: &str = "github.com/e8kor/docs";
const SYNC_KEY: &str = "k-sync";

fn rev(n: u8) -> String {
    format!("{n:x}").repeat(40)
}

#[derive(Clone, Debug)]
struct Version {
    artifact_id: String,
    claim_key: String,
    version: u64,
    state: String,
    retirement: bool,
    workarea_id: String,
    digest: String,
    anchor: Value,
    author: String,
    checks: Vec<Value>,
}

/// The server, as far as a sync can see it.
struct Fake {
    me: RefCell<Me>,
    /// name -> (id, parent id)
    workareas: HashMap<String, (String, Option<String>)>,
    versions: RefCell<Vec<Version>>,
    edges: RefCell<Vec<EdgeRow>>,
    replays: RefCell<HashMap<(String, String), Value>>,
    writes: RefCell<Vec<(String, Value)>>,
    checks: RefCell<Vec<(String, Value)>>,
    /// Hide held rows from the listing, as a run that died before reading
    /// its own writes would see the workarea.
    hide_held: RefCell<bool>,
    /// The size of each batch of source checks.
    batches: RefCell<Vec<usize>>,
}

fn refused(code: &str) -> CliError {
    CliError::Refused {
        status: 409,
        code: code.to_owned(),
        message: code.to_owned(),
        rule: String::new(),
        envelope: Value::Null,
    }
}

impl Fake {
    fn new() -> Self {
        let mut workareas = HashMap::new();
        workareas.insert("design".to_owned(), ("wa-design".to_owned(), None));
        workareas.insert(
            "design-pr-7".to_owned(),
            ("wa-design-pr-7".to_owned(), Some("wa-design".to_owned())),
        );
        Self {
            me: RefCell::new(Me {
                key_id: SYNC_KEY.into(),
                name: "design-sync".into(),
            }),
            workareas,
            versions: RefCell::new(Vec::new()),
            edges: RefCell::new(Vec::new()),
            replays: RefCell::new(HashMap::new()),
            writes: RefCell::new(Vec::new()),
            checks: RefCell::new(Vec::new()),
            hide_held: RefCell::new(false),
            batches: RefCell::new(Vec::new()),
        }
    }

    fn as_key(&self, key_id: &str) {
        *self.me.borrow_mut() = Me {
            key_id: key_id.into(),
            name: key_id.into(),
        };
    }

    fn promote_all(&self) {
        for v in self.versions.borrow_mut().iter_mut() {
            if v.state == "held" {
                v.state = "promoted".into();
            }
        }
        self.writes.borrow_mut().clear();
        self.checks.borrow_mut().clear();
    }

    /// The reviewer rejects every held version.
    fn reject_all(&self) {
        for v in self.versions.borrow_mut().iter_mut() {
            if v.state == "held" {
                v.state = "rejected".into();
            }
        }
        self.reset_calls();
    }

    fn reset_calls(&self) {
        self.writes.borrow_mut().clear();
        self.checks.borrow_mut().clear();
    }

    fn in_subtree(&self, id: &str, root: &str) -> bool {
        let mut cur = Some(id.to_owned());
        while let Some(c) = cur {
            if c == root {
                return true;
            }
            cur = self
                .workareas
                .values()
                .find(|(i, _)| *i == c)
                .and_then(|(_, p)| p.clone());
        }
        false
    }

    fn tip_version(&self, key: &str, workarea: &str) -> u64 {
        self.versions
            .borrow()
            .iter()
            .filter(|v| v.claim_key == key && v.workarea_id == workarea && v.state == "promoted")
            .map(|v| v.version)
            .max()
            .unwrap_or(0)
    }

    fn served(&self, v: &Version) -> Value {
        json!({
            "artifact_id": v.artifact_id, "claim_key": v.claim_key, "version": v.version,
            "tip": v.state == "promoted" && v.version == self.tip_version(&v.claim_key, &v.workarea_id),
            "state": v.state, "retirement": v.retirement, "workarea_id": v.workarea_id,
            "plaintext_digest": v.digest, "anchor": v.anchor,
            "source_checked": v.checks.last().map(|c| json!({
                "rev": c["rev"], "at": "2026-11-05T00:00:00Z", "result": c["result"],
                "moved_to": if c["result"] == "moved" { json!({"path": c["path"], "fragment": c["fragment"]}) } else { Value::Null },
            })),
        })
    }

    fn replay(&self, key: &str) -> Option<Value> {
        self.replays
            .borrow()
            .get(&(self.me.borrow().key_id.clone(), key.to_owned()))
            .cloned()
    }

    fn remember(&self, key: &str, v: &Value) {
        self.replays
            .borrow_mut()
            .insert((self.me.borrow().key_id.clone(), key.to_owned()), v.clone());
    }

    fn capture(&self, claim_key: &str, workarea: &str, version: u64, body: &Value) -> Value {
        let artifact_id = format!("art-{claim_key}-{workarea}-v{version}");
        let text = body["body"].as_str().unwrap_or_default();
        self.versions.borrow_mut().push(Version {
            artifact_id: artifact_id.clone(),
            claim_key: claim_key.to_owned(),
            version,
            state: "held".into(),
            retirement: body["retirement"] == true,
            workarea_id: workarea.to_owned(),
            digest: oml_cli::sync::hash::text_digest(text),
            anchor: body["anchor"].clone(),
            author: self.me.borrow().key_id.clone(),
            checks: Vec::new(),
        });
        json!({ "artifact_id": artifact_id, "state": "held", "version": version })
    }

    /// Seeds a promoted version written by `author`.
    fn seed(&self, author: &str, workarea: &str, claim_key: &str, text: &str, anchor: Value) {
        let version = self.tip_version(claim_key, workarea) + 1;
        self.versions.borrow_mut().push(Version {
            artifact_id: format!("art-{claim_key}-{workarea}-v{version}"),
            claim_key: claim_key.into(),
            version,
            state: "promoted".into(),
            retirement: false,
            workarea_id: workarea.into(),
            digest: oml_cli::sync::hash::text_digest(text),
            anchor,
            author: author.into(),
            checks: Vec::new(),
        });
    }

    fn writes(&self) -> Vec<(String, Value)> {
        self.writes.borrow().clone()
    }

    fn checks(&self) -> Vec<(String, String)> {
        self.checks
            .borrow()
            .iter()
            .map(|(a, b)| (a.clone(), b["result"].as_str().unwrap().to_owned()))
            .collect()
    }
}

impl SyncApi for Fake {
    fn me(&self) -> Result<Me, CliError> {
        Ok(self.me.borrow().clone())
    }

    fn workarea_id(&self, workarea: &str) -> Result<String, CliError> {
        self.workareas
            .get(workarea)
            .map(|(id, _)| id.clone())
            .ok_or_else(|| refused("access.not_found"))
    }

    fn list(&self, q: &ListQuery) -> Result<Vec<Listed>, CliError> {
        let root = self.workarea_id(&q.workarea)?;
        let me = self.me.borrow().key_id.clone();
        let hide = *self.hide_held.borrow();
        Ok(self
            .versions
            .borrow()
            .iter()
            .filter(|v| self.in_subtree(&v.workarea_id, &root))
            .filter(|v| {
                if q.own {
                    v.author == me && !(hide && v.state == "held")
                } else {
                    v.state == "promoted"
                }
            })
            .map(|v| self.served(v))
            .filter(|v| q.own || v["tip"] == true)
            .filter(|v| {
                q.anchor_repo
                    .as_ref()
                    .is_none_or(|r| v["anchor"]["repo"] == r.as_str())
                    && q.anchor_path_prefix.as_ref().is_none_or(|p| {
                        v["anchor"]["path"]
                            .as_str()
                            .is_some_and(|a| a.starts_with(p.as_str()))
                    })
            })
            .map(|v| Listed::of(&v))
            .collect())
    }

    fn submit(&self, body: &Value, key: &str) -> Result<Value, CliError> {
        if let Some(v) = self.replay(key) {
            return Ok(v);
        }
        let workarea = self.workarea_id(body["workarea"].as_str().unwrap())?;
        let claim_key = body["claim_key"].as_str().unwrap();
        if self
            .versions
            .borrow()
            .iter()
            .any(|v| v.claim_key == claim_key && v.workarea_id == workarea && v.state != "rejected")
        {
            return Err(refused("submission.claim_key_conflict"));
        }
        self.writes
            .borrow_mut()
            .push(("submit".into(), body.clone()));
        let answer = self.capture(claim_key, &workarea, 1, body);
        self.remember(key, &answer);
        Ok(answer)
    }

    fn supersede(&self, target: &str, body: &Value, key: &str) -> Result<Value, CliError> {
        if let Some(v) = self.replay(key) {
            return Ok(v);
        }
        let (claim_key, workarea) = {
            let versions = self.versions.borrow();
            let t = versions
                .iter()
                .find(|v| v.artifact_id == target)
                .ok_or_else(|| refused("access.not_found"))?;
            if t.author != self.me.borrow().key_id {
                return Err(refused("access.author_only"));
            }
            (t.claim_key.clone(), t.workarea_id.clone())
        };
        let latest = self
            .versions
            .borrow()
            .iter()
            .filter(|v| v.claim_key == claim_key && v.workarea_id == workarea)
            .filter(|v| matches!(v.state.as_str(), "held" | "promoted"))
            .map(|v| v.version)
            .max()
            .unwrap_or(0);
        if body["expected_version"].as_u64() != Some(latest) {
            return Err(refused("submission.version_conflict"));
        }
        self.writes
            .borrow_mut()
            .push((format!("supersede {target}"), body.clone()));
        let answer = self.capture(&claim_key, &workarea, latest + 1, body);
        self.remember(key, &answer);
        Ok(answer)
    }

    fn source_check(&self, artifact: &str, body: &Value, key: &str) -> Result<Value, CliError> {
        if let Some(v) = self.replay(key) {
            return Ok(v);
        }
        self.checks
            .borrow_mut()
            .push((artifact.to_owned(), body.clone()));
        for v in self.versions.borrow_mut().iter_mut() {
            if v.artifact_id == artifact {
                assert_eq!(v.state, "promoted", "a source check names a promoted tip");
                v.checks.push(body.clone());
            }
        }
        let answer = json!({ "stale": null });
        self.remember(key, &answer);
        Ok(answer)
    }

    fn source_checks(&self, batch: &[(String, Value, String)]) -> Vec<Result<Value, CliError>> {
        self.batches.borrow_mut().push(batch.len());
        batch
            .iter()
            .map(|(a, b, k)| self.source_check(a, b, k))
            .collect()
    }

    fn claim_edges(&self, workarea_id: &str) -> Result<Vec<EdgeRow>, CliError> {
        let _ = workarea_id;
        Ok(self.edges.borrow().clone())
    }

    fn propose_edge(
        &self,
        from: &str,
        to: &str,
        label: &str,
        key: &str,
    ) -> Result<Value, CliError> {
        if let Some(v) = self.replay(key) {
            return Ok(v);
        }
        let id = format!("edge-{}", self.edges.borrow().len() + 1);
        self.edges.borrow_mut().push(EdgeRow {
            edge_id: id.clone(),
            from_artifact_id: from.into(),
            to_artifact_id: to.into(),
            label: label.into(),
            state: "pending".into(),
            proposer_key_id: self.me.borrow().key_id.clone(),
        });
        let answer = json!({ "edge_id": id, "state": "pending" });
        self.remember(key, &answer);
        Ok(answer)
    }

    fn ratify_edge(&self, edge_id: &str, _key: &str) -> Result<Value, CliError> {
        let me = self.me.borrow().key_id.clone();
        for e in self.edges.borrow_mut().iter_mut() {
            if e.edge_id == edge_id {
                if e.proposer_key_id == me {
                    return Err(refused("review.ratifier_is_proposer"));
                }
                e.state = "ratified".into();
            }
        }
        Ok(json!({ "edge_id": edge_id, "state": "ratified" }))
    }
}

/// The source: files and their contents at one commit, through an adapter.
fn loaded(files: &[(&str, &str)], kind: Kind, commit: &str) -> Loaded {
    Loaded {
        requested: Some(kind),
        ..source(files, commit)
    }
}

/// A source built in memory, its adapter not named.
fn source(files: &[(&str, &str)], commit: &str) -> Loaded {
    let mut sorted: Vec<_> = files.to_vec();
    sorted.sort_by_key(|f| f.0);
    Loaded {
        repo: REPO.into(),
        commit: commit.into(),
        path: "docs".into(),
        scope: Scope::Prefix("docs".into()),
        files: sorted
            .iter()
            .map(|(p, t)| ((*p).to_owned(), t.as_bytes().to_vec()))
            .collect(),
        skipped: Vec::new(),
        exclude: Vec::new(),
        requested: None,
        warnings: Vec::new(),
        root: None,
    }
}

fn opts() -> Options {
    Options {
        source: "docs".into(),
        workarea: "design".into(),
        rev: "HEAD".into(),
        max_writes: 200,
        base: "origin/main".into(),
        ..Options::default()
    }
}

fn run(fake: &Fake, src: &Loaded, o: &Options) -> Report {
    let mut sink = Vec::new();
    let mut o = o.clone();
    o.json = false;
    let report = execute(fake, src, &o, &mut sink).unwrap();
    let printed = String::from_utf8(sink).unwrap();
    assert!(
        printed.starts_with("source    github.com/e8kor/docs @ "),
        "{printed}"
    );
    report
}

fn outcomes(report: &Report) -> Vec<(String, String)> {
    report.json["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["outcome"].as_str().unwrap().to_owned(),
                i["claim_key"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

const GUIDE_V1: &str =
    "# Guide\n\n## Alpha\n\nAlpha holds.\n\n## Beta\n\nBeta holds.\n\n## Gamma\n\nGamma holds.\n";
const GUIDE_V2: &str = "# Guide\n\n## Alpha\n\nAlpha  holds.\n\n## Beta\n\nBeta changed.\n\n## Delta\n\nDelta is new.\n";

#[test]
fn the_diff_writes_new_supersedes_changed_retires_removed_and_skips_unchanged() {
    let fake = Fake::new();
    let first = run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &opts(),
    );
    assert_eq!(
        outcomes(&first),
        pairs(&[
            ("new", "doc/docs/guide/alpha"),
            ("new", "doc/docs/guide/beta"),
            ("new", "doc/docs/guide/gamma"),
        ])
    );
    assert_eq!(first.exit, 0);
    let w = fake.writes();
    assert_eq!(w.len(), 3);
    assert_eq!(w[0].1["anchor"]["path"], "docs/guide.md");
    assert_eq!(w[0].1["anchor"]["fragment"], "alpha");
    assert_eq!(w[0].1["anchor"]["rev"], rev(1));
    assert_eq!(w[0].1["anchor"]["hasher"], "oml-md/1");
    assert_eq!(w[0].1["claim_key"], "doc/docs/guide/alpha");
    assert_eq!(w[0].1["workarea"], "design");
    fake.promote_all();

    let second = run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V2)], Kind::Headings, &rev(2)),
        &opts(),
    );
    assert_eq!(
        outcomes(&second),
        pairs(&[
            ("unchanged", "doc/docs/guide/alpha"),
            ("changed", "doc/docs/guide/beta"),
            ("new", "doc/docs/guide/delta"),
            ("removed", "doc/docs/guide/gamma"),
        ])
    );
    let w = fake.writes();
    assert_eq!(w.len(), 3, "{w:?}");
    assert_eq!(w[0].0, "supersede art-doc/docs/guide/beta-wa-design-v1");
    assert_eq!(
        w[0].1["expected_version"], 1,
        "the supersession names the latest version (d19)"
    );
    assert_eq!(w[0].1["anchor"]["rev"], rev(2));
    assert_eq!(w[1].0, "submit");
    assert_eq!(w[2].0, "supersede art-doc/docs/guide/gamma-wa-design-v1");
    assert_eq!(w[2].1["retirement"], true);
    assert!(
        w[2].1["anchor"]["hash"].is_null(),
        "a retirement's anchor carries no hash"
    );
    assert!(
        w[2].1["body"]
            .as_str()
            .unwrap()
            .contains("no longer holds docs/guide.md#gamma")
    );
    let checks: Vec<String> = fake.checks().into_iter().map(|c| c.1).collect();
    assert_eq!(checks, ["fresh", "changed", "removed"]);
}

#[test]
fn a_claim_with_a_held_version_is_pending() {
    let fake = Fake::new();
    run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &opts(),
    );
    fake.reset_calls();
    let second = run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V2)], Kind::Headings, &rev(2)),
        &opts(),
    );
    let got = outcomes(&second);
    assert!(
        got.contains(&("pending".into(), "doc/docs/guide/beta".into())),
        "{got:?}"
    );
    assert!(
        got.contains(&("pending".into(), "doc/docs/guide/gamma".into())),
        "{got:?}"
    );
    assert_eq!(fake.writes().len(), 1, "only delta is new");
    assert!(fake.checks().is_empty(), "a held claim is not checked");
}

#[test]
fn an_unanchored_tip_is_diverged_unless_adopt() {
    let fake = Fake::new();
    fake.seed(
        SYNC_KEY,
        "wa-design",
        "doc/docs/guide/alpha",
        "Old alpha.",
        Value::Null,
    );
    let src = loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1));
    let report = run(&fake, &src, &opts());
    assert!(outcomes(&report).contains(&("diverged".into(), "doc/docs/guide/alpha".into())));
    assert_eq!(report.exit, 1);
    assert_eq!(fake.writes().len(), 2, "the other two are new");
    fake.promote_all();
    let adopted = run(
        &fake,
        &src,
        &Options {
            adopt: true,
            ..opts()
        },
    );
    assert_eq!(
        outcomes(&adopted)[0],
        ("changed".to_owned(), "doc/docs/guide/alpha".to_owned())
    );
    assert_eq!(
        fake.writes()[0].0,
        "supersede art-doc/docs/guide/alpha-wa-design-v1"
    );
}

#[test]
fn a_second_run_at_the_same_rev_writes_nothing() {
    let fake = Fake::new();
    let src = loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1));
    run(&fake, &src, &opts());
    fake.promote_all();
    let again = run(&fake, &src, &opts());
    assert!(
        outcomes(&again).iter().all(|(o, _)| o == "unchanged"),
        "{:?}",
        outcomes(&again)
    );
    assert!(fake.writes().is_empty());
    assert_eq!(again.exit, 0);
}

#[test]
fn a_retried_run_replays_by_idempotency_key() {
    let fake = Fake::new();
    let src = loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1));
    let first = run(&fake, &src, &opts());
    // The retry does not see the first run's held rows, so it plans the
    // same writes under the same keys, and the server answers each from
    // its idempotency record.
    *fake.hide_held.borrow_mut() = true;
    let retry = run(&fake, &src, &opts());
    assert_eq!(first.json["items"], retry.json["items"]);
    assert_eq!(
        fake.versions.borrow().len(),
        3,
        "no write was captured twice"
    );
    let key = first.json["items"][0]["idempotency_key"].as_str().unwrap();
    assert!(
        key.starts_with("sync:design:doc/docs/guide/alpha:new:"),
        "{key}"
    );
    assert_eq!(
        key.len(),
        "sync:design:doc/docs/guide/alpha:new:".len() + 16
    );
}

#[test]
fn a_run_writes_at_most_max_writes_and_the_next_continues() {
    let fake = Fake::new();
    let src = loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1));
    let capped = Options {
        max_writes: 2,
        ..opts()
    };
    let first = run(&fake, &src, &capped);
    assert_eq!(first.json["summary"]["new"], 3);
    assert_eq!(first.json["summary"]["writes_this_run"], 2);
    assert_eq!(fake.writes().len(), 2);
    fake.reset_calls();
    let second = run(&fake, &src, &capped);
    assert_eq!(second.json["summary"]["new"], 1);
    assert_eq!(second.json["summary"]["pending"], 2);
    assert_eq!(fake.writes().len(), 1);
}

#[test]
fn the_dry_run_plan_equals_the_runs() {
    let fake = Fake::new();
    let src = loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1));
    let dry = run(
        &fake,
        &src,
        &Options {
            dry_run: true,
            ..opts()
        },
    );
    assert!(fake.writes().is_empty() && fake.checks().is_empty());
    let real = run(&fake, &src, &opts());
    assert_eq!(dry.json["items"], real.json["items"]);
    assert_eq!(
        serde_json::to_string(&dry.json["items"]).unwrap(),
        serde_json::to_string(&real.json["items"]).unwrap()
    );
}

#[test]
fn a_renamed_file_is_moved_and_writes_nothing() {
    // headings: the keys follow the path, so the move is found by hash.
    let fake = Fake::new();
    run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &opts(),
    );
    fake.promote_all();
    let src = loaded(&[("docs/manual.md", GUIDE_V1)], Kind::Headings, &rev(2));
    let moved = run(&fake, &src, &opts());
    assert_eq!(
        outcomes(&moved),
        pairs(&[
            ("moved", "doc/docs/guide/alpha"),
            ("moved", "doc/docs/guide/beta"),
            ("moved", "doc/docs/guide/gamma"),
        ])
    );
    assert_eq!(moved.exit, 0);
    assert!(fake.writes().is_empty(), "nothing goes back through review");
    let c = fake.checks.borrow()[0].1.clone();
    assert_eq!(
        (
            c["result"].as_str(),
            c["path"].as_str(),
            c["fragment"].as_str()
        ),
        (Some("moved"), Some("docs/manual.md"), Some("alpha"))
    );
    fake.reset_calls();
    // The next run locates each claim at its moved_to and writes nothing.
    let again = run(&fake, &src, &opts());
    assert!(
        outcomes(&again).iter().all(|(o, _)| o == "unchanged"),
        "{:?}",
        outcomes(&again)
    );
    assert!(fake.writes().is_empty());

    // adr: the key survives the rename, and the path alone moved.
    let fake = Fake::new();
    let record = "# ADR-0002 — Two\n\n**Status:** accepted\n\n## Context\n\nWhy.\n\n## Decision\n\n- **d1 — One.** Body.\n";
    run(
        &fake,
        &loaded(&[("docs/ADR-0002-two.md", record)], Kind::Adr, &rev(1)),
        &opts(),
    );
    fake.promote_all();
    let renamed = run(
        &fake,
        &loaded(
            &[("docs/ADR-0002-two-renamed.md", record)],
            Kind::Adr,
            &rev(2),
        ),
        &opts(),
    );
    assert_eq!(
        outcomes(&renamed),
        pairs(&[("moved", "adr-0002-summary"), ("moved", "adr-0002-d1")])
    );
    assert!(fake.writes().is_empty());
}

#[test]
fn a_moved_claim_changed_later_is_superseded_at_its_new_location() {
    let fake = Fake::new();
    run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &opts(),
    );
    fake.promote_all();
    run(
        &fake,
        &loaded(&[("docs/manual.md", GUIDE_V1)], Kind::Headings, &rev(2)),
        &opts(),
    );
    fake.reset_calls();
    let edited = GUIDE_V1.replace("Beta holds.", "Beta now differs.");
    let report = run(
        &fake,
        &loaded(&[("docs/manual.md", &edited)], Kind::Headings, &rev(3)),
        &opts(),
    );
    let got = outcomes(&report);
    assert!(
        got.contains(&("changed".into(), "doc/docs/guide/beta".into())),
        "{got:?}"
    );
    let w = fake.writes();
    assert_eq!(w.len(), 1);
    assert_eq!(
        w[0].0, "supersede art-doc/docs/guide/beta-wa-design-v1",
        "the claim keeps its key"
    );
    assert_eq!(w[0].1["anchor"]["path"], "docs/manual.md");
}

#[test]
fn a_rollback_to_an_earlier_promoted_hash_is_never_written() {
    let fake = Fake::new();
    let v1 = loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1));
    run(&fake, &v1, &opts());
    fake.promote_all();
    let edited = GUIDE_V1.replace("Beta holds.", "Beta changed.");
    run(
        &fake,
        &loaded(&[("docs/guide.md", &edited)], Kind::Headings, &rev(2)),
        &opts(),
    );
    fake.promote_all();
    // A checkout of the old commit.
    let stale = run(&fake, &v1, &opts());
    assert!(outcomes(&stale).contains(&("rollback".into(), "doc/docs/guide/beta".into())));
    assert_eq!(stale.exit, 1);
    assert!(fake.writes().is_empty());
    let note = stale.json["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["outcome"] == "rollback")
        .unwrap()["note"]
        .clone();
    assert!(note.as_str().unwrap().contains("version 1"), "{note}");
    let revert = run(
        &fake,
        &v1,
        &Options {
            allow_rollback: true,
            ..opts()
        },
    );
    assert!(outcomes(&revert).contains(&("changed".into(), "doc/docs/guide/beta".into())));
    assert_eq!(fake.writes().len(), 1);
    assert_eq!(fake.writes()[0].1["expected_version"], 2);
}

#[test]
fn a_sync_of_a_parent_never_retires_a_childs_claims() {
    let fake = Fake::new();
    let draft = loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1));
    run(
        &fake,
        &draft,
        &Options {
            pr: Some(7),
            ..opts()
        },
    );
    fake.promote_all();
    assert!(
        fake.versions
            .borrow()
            .iter()
            .all(|v| v.workarea_id == "wa-design-pr-7")
    );
    let main = run(
        &fake,
        &loaded(
            &[("docs/other.md", "# O\n\n## Only\n\nText.\n")],
            Kind::Headings,
            &rev(2),
        ),
        &opts(),
    );
    assert_eq!(outcomes(&main), pairs(&[("new", "doc/docs/other/only")]));
    assert_eq!(fake.writes().len(), 1);
}

#[test]
fn retire_all_proposes_retiring_every_claim_the_key_wrote() {
    let fake = Fake::new();
    let draft = Options {
        pr: Some(7),
        ..opts()
    };
    run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &draft,
    );
    fake.promote_all();
    // Another key's claim in the same workarea is not this key's to retire.
    fake.seed(
        "k-other",
        "wa-design-pr-7",
        "doc/docs/guide/zeta",
        "Zeta.",
        json!({"repo": REPO, "rev": rev(1), "path": "docs/guide.md", "fragment": "zeta"}),
    );
    let src = loaded(&[], Kind::Headings, &rev(1));
    let retired = run(
        &fake,
        &src,
        &Options {
            retire_all: true,
            ..draft
        },
    );
    assert_eq!(
        outcomes(&retired),
        pairs(&[
            ("removed", "doc/docs/guide/alpha"),
            ("removed", "doc/docs/guide/beta"),
            ("removed", "doc/docs/guide/gamma"),
        ])
    );
    let w = fake.writes();
    assert_eq!(w.len(), 3);
    assert!(w.iter().all(|(_, b)| b["retirement"] == true));
    assert!(
        fake.checks().is_empty(),
        "retiring a draft states nothing about the source"
    );
    assert!(
        retired.json["items"][0]["idempotency_key"]
            .as_str()
            .unwrap()
            .contains(":retire-all:")
    );
}

#[test]
fn check_exits_1_on_stale_removed_or_untracked() {
    let fake = Fake::new();
    let v1 = loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1));
    run(&fake, &v1, &opts());
    fake.promote_all();
    let check = Options {
        check: true,
        ..opts()
    };
    let fresh = run(&fake, &v1, &check);
    assert_eq!(fresh.exit, 0);
    assert!(fake.checks().is_empty(), "--check alone records nothing");
    let report = run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V2)], Kind::Headings, &rev(2)),
        &check,
    );
    assert_eq!(report.exit, 1);
    let states: Vec<(String, String)> = report.json["claims"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["state"].as_str().unwrap().into(),
                c["claim_key"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        states,
        pairs(&[
            ("fresh", "doc/docs/guide/alpha"),
            ("stale", "doc/docs/guide/beta"),
            ("untracked", "doc/docs/guide/delta"),
            ("removed", "doc/docs/guide/gamma"),
        ])
    );
    assert!(fake.writes().is_empty());
    // --record, under another key (a CI key reads the promoted tips).
    fake.as_key("k-ci");
    let recorded = run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V2)], Kind::Headings, &rev(2)),
        &Options {
            record: true,
            ..check
        },
    );
    assert_eq!(recorded.exit, 1);
    let checks: Vec<String> = fake.checks().into_iter().map(|c| c.1).collect();
    assert_eq!(checks, ["fresh", "changed", "removed"]);
}

const ADR_1: &str = "# ADR-0001 — One\n\n**Status:** accepted\n\n## Context\n\nWhy one.\n\n## Decision\n\n- **d1 — First.** One.\n- **d2 — Second.** Two.\n";
const ADR_2: &str = "# ADR-0002 — Two\n\n**Status:** accepted\n\n**Amends:** ADR-0001 (d2), ADR-0099 (d1)\n\n## Context\n\nWhy two.\n\n## Decision\n\n- **d1 — First.** One.\n";

#[test]
fn edges_are_proposed_only_between_promoted_tips_under_a_dreamer_key() {
    let fake = Fake::new();
    let src = loaded(
        &[
            ("docs/ADR-0001-one.md", ADR_1),
            ("docs/ADR-0002-two.md", ADR_2),
        ],
        Kind::Adr,
        &rev(1),
    );
    let plan = run(&fake, &src, &opts());
    let edges = plan.json["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 2);
    assert_eq!(
        edges[0],
        json!({"from": "adr-0002-summary", "label": "amends", "to": "adr-0001-d2", "state": "waiting"})
    );
    assert_eq!(
        edges[1]["state"], "unresolved",
        "ADR-0099 is in neither the source nor the workarea"
    );
    // Before review, both ends are held: nothing to propose.
    fake.as_key("k-reviewer");
    let waiting = run(
        &fake,
        &src,
        &Options {
            edges: true,
            ..opts()
        },
    );
    assert!(fake.edges.borrow().is_empty());
    assert_eq!(waiting.json["edges"][0]["state"], "waiting");
    fake.promote_all();
    let proposed = run(
        &fake,
        &src,
        &Options {
            edges: true,
            ..opts()
        },
    );
    assert_eq!(proposed.json["edges"][0]["state"], "pending");
    assert_eq!(proposed.json["edges"][1]["state"], "unresolved");
    assert_eq!(
        fake.edges.borrow().len(),
        1,
        "an unresolved target is not proposed"
    );
    let e = fake.edges.borrow()[0].clone();
    assert_eq!(
        (e.from_artifact_id.as_str(), e.to_artifact_id.as_str()),
        (
            "art-adr-0002-summary-wa-design-v1",
            "art-adr-0001-d2-wa-design-v1"
        )
    );
    // A second run proposes nothing new.
    run(
        &fake,
        &src,
        &Options {
            edges: true,
            ..opts()
        },
    );
    assert_eq!(fake.edges.borrow().len(), 1);
}

#[test]
fn ratify_ratifies_only_pending_edges_the_source_states() {
    let fake = Fake::new();
    let src = loaded(
        &[
            ("docs/ADR-0001-one.md", ADR_1),
            ("docs/ADR-0002-two.md", ADR_2),
        ],
        Kind::Adr,
        &rev(1),
    );
    run(&fake, &src, &opts());
    fake.promote_all();
    fake.as_key("k-reviewer");
    run(
        &fake,
        &src,
        &Options {
            edges: true,
            ..opts()
        },
    );
    // An edge between the same tips the source does not state.
    fake.edges.borrow_mut().push(EdgeRow {
        edge_id: "edge-x".into(),
        from_artifact_id: "art-adr-0001-d1-wa-design-v1".into(),
        to_artifact_id: "art-adr-0002-d1-wa-design-v1".into(),
        label: "amends".into(),
        state: "pending".into(),
        proposer_key_id: "k-reviewer".into(),
    });
    let ratify = Options {
        edges: true,
        ratify: true,
        ..opts()
    };
    // The proposer does not ratify its own proposal: it is not sent.
    let own = run(&fake, &src, &ratify);
    assert_eq!(
        own.exit, 0,
        "no ratification was sent and refused: {}",
        own.json
    );
    assert!(fake.edges.borrow().iter().all(|e| e.state == "pending"));
    fake.as_key("k-second");
    let report = run(&fake, &src, &ratify);
    let states: Vec<(String, String)> = fake
        .edges
        .borrow()
        .iter()
        .map(|e| (e.edge_id.clone(), e.state.clone()))
        .collect();
    assert_eq!(
        states,
        pairs(&[("edge-1", "ratified"), ("edge-x", "pending")])
    );
    let unstated = report.json["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["edge_id"] == "edge-x")
        .unwrap();
    assert_eq!(unstated["state"], "unstated");
}

#[test]
fn a_tip_written_by_another_key_is_foreign() {
    let fake = Fake::new();
    fake.seed("k-other", "wa-design", "doc/docs/guide/alpha", "Alpha holds.", json!({"repo": REPO, "rev": rev(1), "path": "docs/guide.md", "fragment": "alpha", "hash": "sha256:00", "hasher": "oml-md/1"}));
    let report = run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &opts(),
    );
    assert_eq!(
        outcomes(&report)[0],
        ("foreign".to_owned(), "doc/docs/guide/alpha".to_owned())
    );
    assert_eq!(report.exit, 1);
    assert_eq!(fake.writes().len(), 2);
}

// ---------------------------------------------------------------------------
// The review of #68: H1-H4, M1-M5 and the surviving mutations.
// ---------------------------------------------------------------------------

fn scratch_git(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-syncdiff-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(
        &dir,
        &["remote", "add", "origin", "git@github.com:e8kor/docs.git"],
    );
    dir
}

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn commit_guide(dir: &std::path::Path, text: &str) -> String {
    std::fs::write(dir.join("docs/guide.md"), text).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "guide"]);
    git(dir, &["rev-parse", "HEAD"])
}

fn try_run(fake: &Fake, src: &Loaded, o: &Options) -> Result<Report, String> {
    let mut sink = Vec::new();
    execute(fake, src, o, &mut sink).map_err(|e| match e {
        CliError::Local(m) | CliError::Unanswered(m) => m,
        CliError::Refused { code, .. } => code,
    })
}

#[test]
fn a_run_from_an_older_checkout_is_refused_before_any_write() {
    let dir = scratch_git("stale");
    let first = commit_guide(&dir, GUIDE_V1);
    let second = commit_guide(&dir, GUIDE_V2);
    let fake = Fake::new();
    let at = |rev: &str| {
        oml_cli::sync::run::load(
            &dir,
            &Options {
                rev: rev.into(),
                adapter: Some(Kind::Headings),
                ..opts()
            },
        )
        .unwrap()
    };
    run(&fake, &at(&first), &opts());
    fake.promote_all();
    run(&fake, &at(&second), &opts());
    fake.promote_all();
    let versions = fake.versions.borrow().len();
    let err = try_run(&fake, &at(&first), &opts()).unwrap_err();
    assert!(
        err.contains("older than") && err.contains("--allow-rollback"),
        "{err}"
    );
    assert!(
        fake.writes().is_empty() && fake.checks().is_empty(),
        "nothing is sent"
    );
    assert_eq!(fake.versions.borrow().len(), versions);
    // A deliberate revert is allowed by name.
    let revert = try_run(
        &fake,
        &at(&first),
        &Options {
            allow_rollback: true,
            ..opts()
        },
    )
    .unwrap();
    assert!(outcomes(&revert).iter().any(|(o, _)| o == "changed"));
    // A rev the clone does not know is refused too: fetch first.
    let other = scratch_git("stale-other");
    commit_guide(&other, GUIDE_V1);
    let err = try_run(
        &fake,
        &oml_cli::sync::run::load(
            &other,
            &Options {
                adapter: Some(Kind::Headings),
                ..opts()
            },
        )
        .unwrap(),
        &opts(),
    )
    .unwrap_err();
    assert!(err.contains("unknown to this clone"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn check_reports_another_keys_anchored_memory_as_foreign_and_never_marks_it() {
    let fake = Fake::new();
    let v1 = loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1));
    run(&fake, &v1, &opts());
    fake.promote_all();
    // An agent's memory cites a section: its key is no adapter's.
    fake.seed(
        "k-agent",
        "wa-design",
        "auto-1234",
        "Alpha matters.",
        json!({"repo": REPO, "rev": rev(1), "path": "docs/guide.md", "fragment": "alpha"}),
    );
    fake.seed("k-agent", "wa-design", "auto-5678", "A gone section.", json!({"repo": REPO, "rev": rev(1), "path": "docs/guide.md", "fragment": "zeta", "hash": format!("sha256:{}", "0".repeat(64)), "hasher": "oml-md/1"}));
    let check = Options {
        check: true,
        record: true,
        ..opts()
    };
    let report = run(&fake, &v1, &check);
    assert_eq!(
        report.exit, 0,
        "another key's memory never fails --check: {}",
        report.json
    );
    let claims = report.json["claims"].as_array().unwrap();
    for key in ["auto-1234", "auto-5678"] {
        let c = claims
            .iter()
            .find(|c| c["claim_key"] == key)
            .unwrap_or_else(|| panic!("{key} reported"));
        assert_eq!(c["state"], "foreign", "{c}");
    }
    let marked: Vec<String> = fake.checks().into_iter().map(|c| c.0).collect();
    assert!(
        marked.iter().all(|a| !a.contains("auto-")),
        "--record marks only this sync's claims: {marked:?}"
    );
}

#[test]
fn a_new_file_never_changes_the_recorded_adapter() {
    let fake = Fake::new();
    run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &opts(),
    );
    fake.promote_all();
    let record = "# ADR-0001 — One\n\n**Status:** accepted\n\n## Context\n\nWhy.\n\n## Decision\n\n- **d1 — One.** Body.\n";
    let files = [
        ("docs/guide.md", GUIDE_V1),
        ("docs/ADR-0001-one.md", record),
    ];
    // Without --adapter the workarea's recorded adapter holds.
    let report = run(&fake, &source(&files, &rev(2)), &opts());
    assert_eq!(report.json["source"]["adapter"], "headings");
    assert_eq!(report.json["summary"]["removed"], 0, "{}", report.json);
    fake.promote_all();
    // Naming another adapter is refused unless the change is explicit.
    let adr = Loaded {
        requested: Some(Kind::Adr),
        ..source(&files, &rev(2))
    };
    let err = try_run(&fake, &adr, &opts()).unwrap_err();
    assert!(err.contains("--change-adapter"), "{err}");
    assert!(fake.writes().is_empty());
    let changed = try_run(
        &fake,
        &adr,
        &Options {
            change_adapter: true,
            ..opts()
        },
    )
    .unwrap();
    assert_eq!(changed.json["source"]["adapter"], "adr");
}

#[test]
fn retire_all_without_pr_is_refused() {
    let fake = Fake::new();
    run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &opts(),
    );
    fake.promote_all();
    let err = try_run(
        &fake,
        &loaded(&[], Kind::Headings, &rev(1)),
        &Options {
            retire_all: true,
            ..opts()
        },
    )
    .unwrap_err();
    assert!(err.contains("--pr"), "{err}");
    assert!(fake.writes().is_empty());
}

#[test]
fn an_unreadable_files_claims_are_skipped_never_retired() {
    let fake = Fake::new();
    run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &opts(),
    );
    fake.promote_all();
    let mut broken = loaded(&[], Kind::Headings, &rev(2));
    broken.files = vec![("docs/guide.md".into(), vec![0xff, 0xfe, b'#'])];
    let report = run(&fake, &broken, &opts());
    assert_eq!(report.json["summary"]["removed"], 0, "{}", report.json);
    assert!(fake.writes().is_empty());
    assert!(report.json["unreadable"][0]["path"] == "docs/guide.md");
}

#[test]
fn a_claim_whose_versions_were_all_rejected_is_new_again() {
    let fake = Fake::new();
    let src = loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1));
    let first = run(&fake, &src, &opts());
    fake.reject_all();
    let again = run(&fake, &src, &opts());
    assert!(
        outcomes(&again).iter().all(|(o, _)| o == "new"),
        "{:?}",
        outcomes(&again)
    );
    assert_eq!(again.exit, 0);
    assert_eq!(fake.writes().len(), 3, "re-proposed, not replayed");
    assert_ne!(
        first.json["items"][0]["idempotency_key"],
        again.json["items"][0]["idempotency_key"]
    );
}

#[test]
fn moved_pairs_only_claims_under_the_source_and_needs_equal_text() {
    // The same section under another source is not this run's to move.
    let fake = Fake::new();
    let other = Loaded {
        path: "other".into(),
        scope: Scope::Prefix("other".into()),
        ..loaded(&[("other/guide.md", GUIDE_V1)], Kind::Headings, &rev(1))
    };
    run(
        &fake,
        &other,
        &Options {
            source: "other".into(),
            ..opts()
        },
    );
    fake.promote_all();
    let docs = run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &opts(),
    );
    assert!(
        outcomes(&docs).iter().all(|(o, _)| o == "new"),
        "{:?}",
        outcomes(&docs)
    );
    // A renamed file whose title changed: the spans hash alike, the texts
    // differ (the title is in each), so nothing is moved.
    let fake = Fake::new();
    run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &opts(),
    );
    fake.promote_all();
    let retitled = GUIDE_V1.replace("# Guide", "# Manual");
    let report = run(
        &fake,
        &loaded(&[("docs/manual.md", &retitled)], Kind::Headings, &rev(2)),
        &opts(),
    );
    assert_eq!(
        report.json["summary"]["moved"],
        0,
        "{:?}",
        outcomes(&report)
    );
    assert_eq!(report.json["summary"]["new"], 3);
    assert_eq!(report.json["summary"]["removed"], 3);
}

#[test]
fn a_changed_idempotency_key_names_the_expected_version_and_is_stable() {
    let fake = Fake::new();
    run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &opts(),
    );
    fake.promote_all();
    let v2 = loaded(&[("docs/guide.md", GUIDE_V2)], Kind::Headings, &rev(2));
    let a = run(
        &fake,
        &v2,
        &Options {
            dry_run: true,
            ..opts()
        },
    );
    let b = run(
        &fake,
        &v2,
        &Options {
            dry_run: true,
            ..opts()
        },
    );
    assert_eq!(a.json["items"], b.json["items"], "the plan is stable");
    let beta = a.json["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["outcome"] == "changed")
        .unwrap()
        .clone();
    let hash = beta["anchor"]["hash"].as_str().unwrap();
    assert_eq!(
        beta["idempotency_key"],
        format!(
            "sync:design:doc/docs/guide/beta:changed:v1:{}",
            &hash["sha256:".len().."sha256:".len() + 16]
        )
    );
}

#[test]
fn removal_reaches_only_claims_under_the_source_in_this_repository() {
    let fake = Fake::new();
    let anchor = |repo: &str, path: &str| json!({"repo": repo, "rev": rev(1), "path": path, "fragment": "x", "hash": format!("sha256:{}", "1".repeat(64)), "hasher": "oml-md/1"});
    fake.seed(
        SYNC_KEY,
        "wa-design",
        "doc/other/guide/x",
        "Other.",
        anchor(REPO, "other/guide.md"),
    );
    fake.seed(
        SYNC_KEY,
        "wa-design",
        "doc/docs/guide/x",
        "Elsewhere.",
        anchor("github.com/e8kor/elsewhere", "docs/guide.md"),
    );
    let report = run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &opts(),
    );
    assert_eq!(
        report.json["summary"]["removed"],
        0,
        "{:?}",
        outcomes(&report)
    );
    assert!(fake.writes().iter().all(|(_, b)| b["retirement"] != true));
}

#[test]
fn unchanged_needs_an_equal_text_as_well_as_an_equal_hash() {
    let fake = Fake::new();
    let src = loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1));
    let alpha = src.files.clone();
    let x = Kind::Headings
        .adapter()
        .extract("docs/guide.md", &alpha[0].1)
        .unwrap();
    let a = &x.items[0];
    // Same span hash, another text (a tip written by an older extractor).
    fake.seed(SYNC_KEY, "wa-design", &a.claim_key, "An older extraction.", json!({"repo": REPO, "rev": rev(1), "path": "docs/guide.md", "fragment": "alpha", "hash": a.hash, "hasher": "oml-md/1"}));
    let report = run(&fake, &src, &opts());
    assert_eq!(
        outcomes(&report)[0],
        ("changed".to_owned(), a.claim_key.clone())
    );
}

#[test]
fn check_exits_1_on_untracked_alone() {
    let fake = Fake::new();
    let report = run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &Options {
            check: true,
            ..opts()
        },
    );
    assert_eq!(report.exit, 1);
    assert!(
        report.json["claims"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["state"] == "untracked")
    );
}

#[test]
fn an_unresolved_target_is_reported_and_not_proposed() {
    let fake = Fake::new();
    let src = loaded(&[("docs/ADR-0002-two.md", ADR_2)], Kind::Adr, &rev(1));
    run(&fake, &src, &opts());
    fake.promote_all();
    fake.as_key("k-reviewer");
    let report = run(
        &fake,
        &src,
        &Options {
            edges: true,
            ..opts()
        },
    );
    let states: Vec<&str> = report.json["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["state"].as_str().unwrap())
        .collect();
    assert_eq!(
        states,
        ["unresolved", "unresolved"],
        "ADR-0001 and ADR-0099 are in neither the source nor the workarea"
    );
    assert!(fake.edges.borrow().is_empty());
}

#[test]
fn source_checks_go_out_together_before_the_writes() {
    let fake = Fake::new();
    run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V1)], Kind::Headings, &rev(1)),
        &opts(),
    );
    fake.promote_all();
    run(
        &fake,
        &loaded(&[("docs/guide.md", GUIDE_V2)], Kind::Headings, &rev(2)),
        &opts(),
    );
    assert_eq!(
        fake.batches.borrow().as_slice(),
        [3],
        "one batch of three checks"
    );
}

#[test]
fn an_excluded_path_is_out_of_the_source_never_read_or_retired() {
    let record = "# ADR-0001 — One\n\n**Status:** accepted\n\n## Context\n\nWhy.\n\n## Decision\n\n- **d1 — One.** Body.\n";
    let files = [
        ("docs/guide.md", GUIDE_V1),
        ("docs/adr/ADR-0001-one.md", record),
    ];
    let fake = Fake::new();
    // docs/ with headings, docs/adr/** left to the adr workarea.
    let headings = Loaded {
        exclude: vec!["docs/adr/**".into()],
        ..loaded(&files, Kind::Headings, &rev(1))
    };
    let first = run(&fake, &headings, &opts());
    assert!(
        outcomes(&first)
            .iter()
            .all(|(_, k)| k.starts_with("doc/docs/guide/")),
        "the excluded record is not read: {:?}",
        outcomes(&first)
    );
    fake.promote_all();
    // A claim this sync wrote under docs/adr earlier (before the exclude)
    // is out of the source: neither compared nor retired.
    fake.seed(
        SYNC_KEY,
        "wa-design",
        "doc/docs/adr/adr-0001-one-0a1b2c3d/one",
        "One.",
        json!({"repo": REPO, "rev": rev(1), "path": "docs/adr/ADR-0001-one.md", "fragment": "d1-one", "hash": format!("sha256:{}", "2".repeat(64)), "hasher": "oml-md/1"}),
    );
    let report = run(
        &fake,
        &headings,
        &Options {
            change_exclude: true,
            ..opts()
        },
    );
    assert_eq!(
        report.json["summary"]["removed"],
        0,
        "{:?}",
        outcomes(&report)
    );
    assert!(fake.writes().is_empty());
}

#[test]
fn excluding_a_path_that_holds_this_syncs_claims_is_refused_unless_explicit() {
    let record = "# ADR-0001 — One\n\n**Status:** accepted\n\n## Context\n\nWhy.\n\n## Decision\n\n- **d1 — One.** Body.\n";
    let files = [
        ("docs/guide.md", GUIDE_V1),
        ("docs/adr/ADR-0001-one.md", record),
    ];
    let fake = Fake::new();
    run(&fake, &loaded(&files, Kind::Headings, &rev(1)), &opts());
    fake.promote_all();
    let narrowed = Loaded {
        exclude: vec!["docs/adr/**".into()],
        ..loaded(&files, Kind::Headings, &rev(1))
    };
    let err = try_run(&fake, &narrowed, &opts()).unwrap_err();
    assert!(
        err.contains("--change-exclude") && err.contains("docs/adr/ADR-0001-one.md"),
        "{err}"
    );
    assert!(fake.writes().is_empty() && fake.checks().is_empty());
    let report = try_run(
        &fake,
        &narrowed,
        &Options {
            change_exclude: true,
            ..opts()
        },
    )
    .unwrap();
    assert_eq!(
        report.json["summary"]["removed"], 0,
        "an excluded claim is left, not retired"
    );
    assert_eq!(report.json["summary"]["unchanged"], 3);
}
