//! The key builder, `/web/keys/new` (ADR-0096 d1 to d3, d10; D5): a person
//! starts from a template, shapes the key on a form, and reviews it in
//! words, as a read-only tree and as the exact request before **Issue key**
//! posts it once to `/web/keys` (`build_key`), which shows the value or the
//! connect code once, as the other issue acts do.
//!
//! Every step is a `GET` with no script. The review step checks every
//! refusal it can decide from the role table and the tree it read (D12);
//! `name.taken`, `limit.quota`, `access.not_found` and
//! `oauth.connect_refused` stay the server's answers, and **Issue key**
//! refused `name.taken`, `limit.quota` or `oauth.connect_refused` comes
//! back to the review step with its notice and the draft. Each `GET` makes
//! `whoami` (the session check) and the tree of the person's root.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::{Query, State};
use axum::response::Response;
use http::{StatusCode, Uri};
use oml_page::web::{ADMIN_KEYS_PATH, USER_KEYS_PATH, attr, hidden, shell, short, text};
use oml_sdk::WorkspaceNode;
use serde_json::{Value, json};

use crate::keys::{
    ADMIN_NEW_KEY_PATH, NEW_KEY_PATH, denied, denied_notice, expiry_sent, rfc3339, unix_now,
};
use crate::pages::{Fields, refused};
use crate::session::{PageCalls, Session};
use crate::{Capacity, Web, acts, outcome};

/// The most grants a session issuance takes, and the most denials per
/// grant (ADR-0096 d1, decided by the operator, 2026-11-24).
pub const GRANTS_MAX: usize = 16;

/// The four session names after `<user>-`, compared whole (ADR-0096 d1).
const SESSION_SUFFIXES: [&str; 4] = ["web", "review", "curate", "admin"];

/// The templates of step one: each fills the form and fixes nothing after
/// issue.
const TEMPLATES: [(&str, &str, &str); 5] = [
    (
        "root",
        "Whole root",
        "Recall and write on the whole root: the key the console issued before v0.8.5.",
    ),
    (
        "one",
        "One workspace",
        "Recall and write on one workspace and below.",
    ),
    (
        "read",
        "Read only",
        "Recall on the whole root; writes nothing.",
    ),
    (
        "write",
        "Write only",
        "A submitter key: writes, and recalls nothing but its own writes.",
    ),
    (
        "connector",
        "Connector for Claude",
        "A connector key: a connect code in place of a value.",
    ),
];

/// The admin builder's templates (ADR-0097 d29): the personal ones less
/// the whole root, and a reviewer key.
const ADMIN_TEMPLATES: [(&str, &str, &str); 6] = [
    (
        "one",
        "One workspace",
        "Recall and write on one workspace and below.",
    ),
    ("read", "Read only", "Recall on one workspace and below."),
    (
        "write",
        "Write only",
        "A submitter key: writes, and recalls nothing but its own writes.",
    ),
    (
        "review",
        "Reviewer",
        "Reviews held memories on one workspace and below.",
    ),
    (
        "new",
        "With a new workspace",
        "Creates a workspace in the same act and writes and recalls there.",
    ),
    (
        "connector",
        "Connector for Claude",
        "A connector key: a connect code in place of a value.",
    ),
];

/// Which builder a page is: the person's own, inside the own root
/// (ADR-0096 d1), or the admin's, anywhere in the tenant (ADR-0097 d29).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Personal,
    Admin,
}

impl Side {
    /// The builder's own page.
    pub(crate) fn path(self) -> &'static str {
        match self {
            Side::Personal => NEW_KEY_PATH,
            Side::Admin => ADMIN_NEW_KEY_PATH,
        }
    }

    /// The Keys page the builder posts to and leads back to.
    pub(crate) fn keys_path(self) -> &'static str {
        match self {
            Side::Personal => USER_KEYS_PATH,
            Side::Admin => ADMIN_KEYS_PATH,
        }
    }
}

/// One grant row of the form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GrantDraft {
    pub workspace: String,
    pub role: String,
    pub covers: String,
    pub deny: Vec<String>,
    /// The key creates child workspaces where the grant reaches (ADR-0097
    /// d31).
    pub create: bool,
}

/// One "new workspace" row: the issuance creates `name` under `parent`
/// and grants the key its role's default grants there (ADR-0097 d30).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NewDraft {
    pub parent: String,
    pub name: String,
}

/// The admin builder's fields beside the personal ones (ADR-0097 d29).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AdminDraft {
    pub user: String,
    /// `person` or `service`; empty is `agent`.
    pub used_by: String,
    pub principal: String,
    pub crew: String,
    pub reason: String,
}

impl AdminDraft {
    /// Who holds the key, `agent` when the draft names no one else.
    fn used_by(&self) -> &str {
        if self.used_by.is_empty() {
            "agent"
        } else {
            &self.used_by
        }
    }
}

/// The form's fields as read from the query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Draft {
    pub connector: bool,
    pub name: String,
    pub role: String,
    pub grants: Vec<GrantDraft>,
    pub new_workspaces: Vec<NewDraft>,
    pub default_workspace: String,
    pub scope: String,
    pub expires: String,
    pub rate: String,
    pub admin: AdminDraft,
}

/// One refusal the review step draws, with the control it names; `code`
/// is the server's code for the same refusal, empty for a check that is
/// the page's own and no server answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Check {
    pub code: &'static str,
    pub message: String,
    pub control: String,
}

impl Draft {
    /// The form a template opens, on the person's root `user`.
    pub(crate) fn template(shape: &str, user: &str) -> Self {
        let grant = |workspace: &str, role: &str| GrantDraft {
            workspace: workspace.to_owned(),
            role: role.to_owned(),
            covers: "subtree".to_owned(),
            ..GrantDraft::default()
        };
        let (role, grants) = match shape {
            "one" => (
                "contributor",
                vec![grant("", "submitter"), grant("", "reader")],
            ),
            "read" => ("reader", vec![grant(user, "reader")]),
            "write" => ("submitter", vec![grant(user, "submitter")]),
            "review" => ("reviewer", vec![grant(user, "reviewer")]),
            "new" => ("contributor", Vec::new()),
            _ => (
                "contributor",
                vec![grant(user, "submitter"), grant(user, "reader")],
            ),
        };
        let new_workspaces = if shape == "new" {
            vec![NewDraft {
                parent: user.to_owned(),
                name: String::new(),
            }]
        } else {
            Vec::new()
        };
        Self {
            connector: shape == "connector",
            role: role.to_owned(),
            grants,
            new_workspaces,
            scope: "ancestors".to_owned(),
            ..Self::default()
        }
    }

    /// The draft the query names; repeated `g<i>d` fields are one grant's
    /// Except list, any other field sent twice is refused.
    pub(crate) fn of_query(pairs: &[(String, String)]) -> Result<Self, String> {
        let mut single: BTreeMap<&str, &str> = BTreeMap::new();
        let mut deny: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (name, value) in pairs {
            if let Some(i) = grant_index(name, 'd') {
                if !value.is_empty() {
                    deny.entry(i).or_default().push(value.clone());
                }
                continue;
            }
            if single.insert(name.as_str(), value.as_str()).is_some() {
                return Err(format!("the field {name:?} is sent more than once"));
            }
        }
        let get = |n: &str| single.get(n).copied().unwrap_or("").to_owned();
        let count: usize = single
            .get("n")
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
            .min(GRANTS_MAX + 1);
        let mut grants = Vec::new();
        for i in 0..count {
            if single.contains_key(format!("x{i}").as_str()) {
                continue;
            }
            grants.push(GrantDraft {
                workspace: get(&format!("g{i}w")),
                role: get(&format!("g{i}r")),
                covers: match get(&format!("g{i}c")).as_str() {
                    "node" => "node".to_owned(),
                    _ => "subtree".to_owned(),
                },
                deny: deny.remove(&i).unwrap_or_default(),
                create: get(&format!("g{i}k")) == "1",
            });
        }
        if single.contains_key("add") {
            grants.push(GrantDraft {
                role: "reader".to_owned(),
                covers: "subtree".to_owned(),
                ..GrantDraft::default()
            });
        }
        let created: usize = single
            .get("m")
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
            .min(GRANTS_MAX + 1);
        let mut new_workspaces = Vec::new();
        for i in 0..created {
            if single.contains_key(format!("y{i}").as_str()) {
                continue;
            }
            new_workspaces.push(NewDraft {
                parent: get(&format!("w{i}p")),
                name: get(&format!("w{i}n")),
            });
        }
        if single.contains_key("addw") {
            new_workspaces.push(NewDraft::default());
        }
        Ok(Self {
            connector: get("kind") == "connector",
            name: get("name"),
            role: match get("role").as_str() {
                r @ ("reader" | "submitter" | "reviewer") => r.to_owned(),
                _ => "contributor".to_owned(),
            },
            grants,
            new_workspaces,
            default_workspace: get("dw"),
            scope: match get("gs").as_str() {
                "workspace" => "workspace".to_owned(),
                _ => "ancestors".to_owned(),
            },
            expires: get("exp"),
            rate: get("rpm"),
            admin: AdminDraft {
                user: get("ku"),
                // Empty is an agent's key, the default, so a personal draft
                // carries no admin field (ADR-0097 d29).
                used_by: match get("kb").as_str() {
                    b @ ("person" | "service") => b.to_owned(),
                    _ => String::new(),
                },
                principal: get("kp"),
                crew: get("kc"),
                reason: get("why"),
            },
        })
    }

    /// The draft as query or form fields, each `g<i>d` once per denial.
    pub(crate) fn fields(&self) -> Vec<(String, String)> {
        let mut out = vec![
            (
                "kind".to_owned(),
                if self.connector { "connector" } else { "agent" }.to_owned(),
            ),
            ("name".to_owned(), self.name.clone()),
            ("role".to_owned(), self.role.clone()),
            ("n".to_owned(), self.grants.len().to_string()),
        ];
        for (i, g) in self.grants.iter().enumerate() {
            out.push((format!("g{i}w"), g.workspace.clone()));
            out.push((format!("g{i}r"), g.role.clone()));
            out.push((format!("g{i}c"), g.covers.clone()));
            for d in &g.deny {
                out.push((format!("g{i}d"), d.clone()));
            }
            if g.create {
                out.push((format!("g{i}k"), "1".to_owned()));
            }
        }
        out.push(("m".to_owned(), self.new_workspaces.len().to_string()));
        for (i, w) in self.new_workspaces.iter().enumerate() {
            out.push((format!("w{i}p"), w.parent.clone()));
            out.push((format!("w{i}n"), w.name.clone()));
        }
        out.push(("dw".to_owned(), self.default_workspace.clone()));
        out.push(("gs".to_owned(), self.scope.clone()));
        out.push(("exp".to_owned(), self.expires.clone()));
        out.push(("rpm".to_owned(), self.rate.clone()));
        let a = &self.admin;
        for (name, value) in [
            ("ku", &a.user),
            ("kb", &a.used_by),
            ("kp", &a.principal),
            ("kc", &a.crew),
            ("why", &a.reason),
        ] {
            if !value.is_empty() {
                out.push((name.to_owned(), value.clone()));
            }
        }
        out
    }

    /// The draft as one form's hidden fields; an Except list is one field,
    /// its workspaces joined by commas, since a form's field is sent once.
    pub(crate) fn hidden(&self) -> String {
        let mut out = String::new();
        let mut joined: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, value) in self.fields() {
            if grant_index(&name, 'd').is_some() {
                joined.entry(name).or_default().push(value);
            } else {
                out.push_str(&hidden(&name, &value));
            }
        }
        for (name, values) in joined {
            out.push_str(&hidden(&name, &values.join(",")));
        }
        out
    }

    /// The `POST /v2/keys` body this draft sends, `user` the person's
    /// name: the grants as sent, the date as the next UTC day's end, the
    /// rate as a number.
    #[cfg(test)]
    pub(crate) fn body(&self, user: &str) -> Value {
        self.body_as(Side::Personal, user)
    }

    /// The `POST /v2/keys` body of `side`'s builder: the personal one names
    /// the key `<user>-<name>`; the admin one sends the name whole, with
    /// the person, `used_by`, principal, crew and reason it was given
    /// (ADR-0097 d29).
    pub(crate) fn body_as(&self, side: Side, user: &str) -> Value {
        let name = match side {
            Side::Personal => format!("{user}-{}", self.name),
            Side::Admin => self.name.clone(),
        };
        let mut body = json!({
            "name": name,
            "role": self.role,
            "connect_code": self.connector,
            "grants": self.grants.iter().map(|g| {
                let mut grant = json!({
                    "workspace": g.workspace,
                    "role": g.role,
                    "covers": g.covers,
                });
                if !g.deny.is_empty() {
                    grant["deny"] = g.deny.iter().map(|d| json!({"workspace": d})).collect();
                }
                if g.create {
                    grant["create"] = json!(true);
                }
                grant
            }).collect::<Vec<_>>(),
            "guardrail_scope": self.scope,
        });
        if !self.new_workspaces.is_empty() {
            body["create_workspaces"] = self
                .new_workspaces
                .iter()
                .map(|w| json!({ "parent": w.parent, "name": w.name }))
                .collect();
        }
        if side == Side::Admin {
            let a = &self.admin;
            body["used_by"] = json!(a.used_by());
            body["principal"] = json!(if a.principal.is_empty() {
                &self.name
            } else {
                &a.principal
            });
            for (field, value) in [("user", &a.user), ("crew", &a.crew), ("reason", &a.reason)] {
                if !value.is_empty() {
                    body[field] = json!(value);
                }
            }
        }
        if !self.default_workspace.is_empty() {
            body["default_workspace"] = json!(self.default_workspace);
        }
        if let Some(at) = expiry_sent(&self.expires) {
            body["expiry"] = json!(at);
        }
        if let Ok(rate) = self.rate.parse::<u64>() {
            body["requests_per_minute"] = json!(rate);
        }
        body
    }

    /// Every refusal the page can decide before **Issue key** (ADR-0096 d1;
    /// D12), in the order the server checks them; `tree` is the person's
    /// root as read, `now` this process's clock in Unix seconds.
    #[cfg(test)]
    pub(crate) fn checks(&self, user: &str, tree: &[WorkspaceNode], now: i64) -> Vec<Check> {
        self.checks_as(Side::Personal, user, tree, now)
    }

    /// [`Self::checks`] on `side`'s builder: the admin's reads the tenant's
    /// tree, takes a reviewer key and requires a reason, since its issuance
    /// is widening (ADR-0097 d26, d29).
    pub(crate) fn checks_as(
        &self,
        side: Side,
        user: &str,
        tree: &[WorkspaceNode],
        now: i64,
    ) -> Vec<Check> {
        let mut out = Vec::new();
        let mut refuse = |code: &'static str, message: String, control: &str| {
            out.push(Check {
                code,
                message,
                control: control.to_owned(),
            });
        };
        let name_ok = !self.name.is_empty()
            && self
                .name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            && !self.name.starts_with('-')
            && !self.name.ends_with('-');
        if !name_ok {
            refuse(
                "request.validation",
                "A name is lower-case letters, digits and hyphens after the prefix.".to_owned(),
                "kn",
            );
        } else if side == Side::Personal && SESSION_SUFFIXES.contains(&self.name.as_str()) {
            refuse(
                "request.validation",
                format!(
                    "{user}-{} is a session key's name: web, review, curate and admin are reserved.",
                    self.name
                ),
                "kn",
            );
        }
        if self.grants.len() > GRANTS_MAX {
            refuse(
                "request.validation",
                format!("A key takes at most {GRANTS_MAX} grants here (too_many)."),
                "gr-h",
            );
        }
        if side == Side::Personal && self.role == "reviewer" {
            refuse(
                "access.issuer_ceiling",
                "A key of the user's own is a contributor, a reader or a submitter.".to_owned(),
                "kro",
            );
        }
        let path_of = |name: &str| tree.iter().find(|n| n.name == name).map(|n| n.path.clone());
        for (i, g) in self.grants.iter().enumerate() {
            let control = format!("g{i}w");
            let Some(path) = path_of(&g.workspace) else {
                let shown = if g.workspace.is_empty() {
                    "no workspace"
                } else {
                    g.workspace.as_str()
                };
                let message = match side {
                    Side::Personal => format!(
                        "Grant {}: {shown} is not a workspace in {user}; a grant lies inside the root.",
                        i + 1
                    ),
                    Side::Admin => {
                        format!("Grant {}: {shown} is not a workspace of the tenant.", i + 1)
                    }
                };
                refuse("access.issuer_ceiling", message, &control);
                continue;
            };
            let admitted = match self.role.as_str() {
                "reader" => g.role == "reader",
                "submitter" => g.role == "submitter",
                "reviewer" => g.role == "reviewer",
                _ => matches!(g.role.as_str(), "reader" | "submitter"),
            };
            if !admitted {
                refuse(
                    "access.issuer_ceiling",
                    format!(
                        "Grant {}: a {} key takes no {} grant.",
                        i + 1,
                        self.role,
                        g.role
                    ),
                    &control,
                );
            }
            if g.deny.len() > GRANTS_MAX {
                refuse(
                    "request.validation",
                    format!(
                        "Grant {}: at most {GRANTS_MAX} workspaces in Except (too_many).",
                        i + 1
                    ),
                    &format!("g{i}d"),
                );
            }
            if g.covers == "node" && !g.deny.is_empty() {
                refuse(
                    "request.validation",
                    format!(
                        "Grant {}: a grant on this workspace only takes no Except.",
                        i + 1
                    ),
                    &format!("g{i}d"),
                );
            }
            for d in &g.deny {
                let inside = path_of(d).is_some_and(|p| below(&p, &path) && p != path);
                if !inside {
                    refuse(
                        "request.validation",
                        format!("Grant {}: {d} is not below {}.", i + 1, g.workspace),
                        &format!("g{i}d"),
                    );
                }
            }
        }
        if self.new_workspaces.len() > GRANTS_MAX {
            refuse(
                "request.validation",
                format!("A key creates at most {GRANTS_MAX} workspaces here (too_many)."),
                "nw-h",
            );
        }
        // A new workspace brings the role's default grants, a reader and a
        // submitter grant for a contributor (ADR-0097 d30).
        for (i, w) in self.new_workspaces.iter().enumerate() {
            if path_of(&w.parent).is_none() {
                refuse(
                    "access.not_found",
                    format!(
                        "New workspace {}: {} is not a workspace {}.",
                        i + 1,
                        if w.parent.is_empty() {
                            "no parent"
                        } else {
                            w.parent.as_str()
                        },
                        match side {
                            Side::Personal => "in the root",
                            Side::Admin => "of the tenant",
                        }
                    ),
                    &format!("w{i}p"),
                );
            }
            let name_ok = !w.name.is_empty()
                && w.name
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                && !w.name.starts_with('-')
                && !w.name.ends_with('-');
            if !name_ok {
                refuse(
                    "request.validation",
                    format!(
                        "New workspace {}: a name is lower-case letters, digits and hyphens.",
                        i + 1
                    ),
                    &format!("w{i}n"),
                );
            }
        }
        if side == Side::Admin && self.grants.is_empty() && self.new_workspaces.is_empty() {
            refuse(
                "request.validation",
                "A key takes a grant or a new workspace.".to_owned(),
                "gr-h",
            );
        }
        if side == Side::Admin && self.admin.reason.trim().is_empty() {
            refuse(
                "request.validation",
                "A reason is required: the issuance waits on another admin, who reads it."
                    .to_owned(),
                "why",
            );
        }
        if self.role == "contributor" && self.new_workspaces.is_empty() {
            let has = |r: &str| self.grants.iter().any(|g| g.role == r);
            if !(has("reader") && has("submitter")) {
                refuse(
                    "request.validation",
                    "A contributor key takes a reader and a submitter grant (contributor_grants)."
                        .to_owned(),
                    "gr-h",
                );
            }
        }
        if !self.default_workspace.is_empty() {
            let covered = path_of(&self.default_workspace).is_some_and(|p| {
                self.grants.iter().any(|g| {
                    path_of(&g.workspace).is_some_and(|gp| {
                        (if g.covers == "node" {
                            p == gp
                        } else {
                            below(&p, &gp)
                        }) && !g
                            .deny
                            .iter()
                            .any(|d| path_of(d).is_some_and(|dp| below(&p, &dp)))
                    })
                })
            });
            if !covered {
                refuse(
                    "request.validation",
                    "The default workspace is covered by no grant, or lies inside an Except."
                        .to_owned(),
                    "kd",
                );
            }
        }
        if !self.expires.is_empty() {
            match expiry_sent(&self.expires) {
                None => refuse(
                    "request.validation",
                    "Expires is a date, YYYY-MM-DD.".to_owned(),
                    "ke",
                ),
                // Over in every time zone: a date before the date of now
                // minus 12 hours. The page's own check: the server reads
                // the instant it is sent, so no code of its names this.
                Some(_) if self.expires.as_str() < &rfc3339(now - 43_200)[..10] => refuse(
                    "",
                    format!("{} is over in every time zone.", self.expires),
                    "ke",
                ),
                Some(_) => {}
            }
        }
        if !self.rate.is_empty()
            && !self
                .rate
                .parse::<u64>()
                .is_ok_and(|n| (1..=1_000_000).contains(&n))
        {
            refuse(
                "request.validation",
                "Requests a minute is a whole number from 1 to 1,000,000.".to_owned(),
                "kr",
            );
        }
        out
    }

    /// The effective access in words, one line per grant.
    pub(crate) fn words(&self) -> Vec<String> {
        let mut out = Vec::new();
        for g in &self.grants {
            let act = match g.role.as_str() {
                "reader" => "recalls",
                "reviewer" => "reviews",
                _ => "writes",
            };
            let reach = if g.covers == "node" {
                format!("in <code>{}</code> only", short(&g.workspace))
            } else {
                format!("in <code>{}</code> and below", short(&g.workspace))
            };
            let except = if g.deny.is_empty() {
                String::new()
            } else {
                format!(
                    ", except {}",
                    g.deny
                        .iter()
                        .map(|d| format!("<code>{}</code>", short(d)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let create = if g.create {
                ", and creates workspaces there"
            } else {
                ""
            };
            out.push(format!("{act} {reach}{except}{create}"));
        }
        for w in &self.new_workspaces {
            let act = match self.role.as_str() {
                "reader" => "recalls",
                "reviewer" => "reviews",
                "submitter" => "writes",
                _ => "recalls and writes",
            };
            out.push(format!(
                "creates <code>{}</code> under <code>{}</code>, and {act} in it and below",
                short(&w.name),
                short(&w.parent)
            ));
        }
        if self.role == "submitter" {
            out.push("Reads nothing in <code>universe</code>.".to_owned());
        } else {
            out.push(
                "Recalls <code>universe</code>'s own memories: the server adds that grant to a contributor or reader key.".to_owned(),
            );
        }
        out
    }

    /// The submitter grants no reader grant reaches: such a contributor is
    /// issued and listed "no recall" there (ADR-0093 d3).
    pub(crate) fn without_recall(&self, tree: &[WorkspaceNode]) -> Vec<String> {
        if self.role != "contributor" {
            return Vec::new();
        }
        let path_of = |name: &str| tree.iter().find(|n| n.name == name).map(|n| n.path.clone());
        self.grants
            .iter()
            .filter(|g| g.role == "submitter")
            .filter(|s| {
                let Some(sp) = path_of(&s.workspace) else {
                    return false;
                };
                !self.grants.iter().any(|r| {
                    r.role == "reader"
                        && path_of(&r.workspace).is_some_and(|rp| {
                            if r.covers == "node" {
                                rp == sp
                            } else {
                                below(&sp, &rp)
                            }
                        })
                })
            })
            .map(|s| s.workspace.clone())
            .collect()
    }

    /// The marks of one node of the tree: recall, write, except, default.
    fn marks(&self, node: &WorkspaceNode, tree: &[WorkspaceNode]) -> Vec<&'static str> {
        let path_of = |name: &str| tree.iter().find(|n| n.name == name).map(|n| n.path.clone());
        let mut marks = Vec::new();
        let mut denied = false;
        for g in &self.grants {
            let Some(gp) = path_of(&g.workspace) else {
                continue;
            };
            let reaches = if g.covers == "node" {
                node.path == gp
            } else {
                below(&node.path, &gp)
            };
            if !reaches {
                continue;
            }
            if g.deny
                .iter()
                .any(|d| path_of(d).is_some_and(|dp| below(&node.path, &dp)))
            {
                denied = true;
                continue;
            }
            let mark = if g.role == "reader" {
                "recall"
            } else {
                "write"
            };
            if !marks.contains(&mark) {
                marks.push(mark);
            }
        }
        // A grant's Except is drawn wherever it reaches, beside what
        // another grant still gives the node.
        if denied {
            marks.push("except");
        }
        if marks.is_empty() {
            marks.push("not covered");
        }
        if node.name == self.default_workspace {
            marks.push("default");
        }
        marks
    }
}

/// `g<i><suffix>`'s index.
fn grant_index(name: &str, suffix: char) -> Option<usize> {
    name.strip_prefix('g')?.strip_suffix(suffix)?.parse().ok()
}

/// Whether `path` is `parent` or lies below it.
fn below(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// `GET /web/keys/new`: the templates, the form (`?shape=` or a posted
/// back draft), or the review (`step=review`).
pub async fn page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    view(&web, &session, &calls, &uri, Side::Personal).await
}

/// `GET /web/admin/keys/new`: the admin key builder, the personal one's
/// steps over the tenant's tree with the admin's fields (ADR-0097 d29).
/// Any session but an admin entry's answers `access.not_found`.
pub async fn admin_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    if !session.admin {
        return refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            &calls,
        );
    }
    view(&web, &session, &calls, &uri, Side::Admin).await
}

async fn view(web: &Web, session: &Session, calls: &PageCalls, uri: &Uri, side: Side) -> Response {
    let Ok(Query(pairs)) = Query::<Vec<(String, String)>>::try_from_uri(uri) else {
        return acts::invalid("the query is not a page's", calls);
    };
    let user = match side {
        Side::Personal => match session.whoami.user.as_ref().map(|u| u.name.clone()) {
            Some(user) => user,
            None => {
                return refused(
                    StatusCode::NOT_FOUND,
                    "access.not_found",
                    "no such page here",
                    calls,
                );
            }
        },
        Side::Admin => String::new(),
    };
    let bar = match side {
        Side::Personal => session.personal(),
        Side::Admin => session.administration(),
    };
    let draw = |title: &str, body: &str| {
        outcome::html(
            StatusCode::OK,
            shell(title, &bar, side.keys_path(), None, body),
        )
    };
    // A restricted session issues nothing: the sentence, and no template.
    if side == Side::Personal && denied(session) {
        return draw(
            "New key",
            &format!(
                "<h1>New key</h1>{}<p><a href=\"{USER_KEYS_PATH}\">Back to Keys</a></p>",
                denied_notice()
            ),
        );
    }
    let shape = pairs
        .iter()
        .find(|(n, _)| n == "shape")
        .map(|(_, v)| v.as_str());
    if pairs.is_empty() {
        return draw("New key", &templates(side, &user));
    }
    let draft = match shape {
        Some(shape) => Draft::template(shape, &user),
        None => match Draft::of_query(&pairs) {
            Ok(draft) => draft,
            Err(message) => return acts::invalid(&message, calls),
        },
    };
    // The person's root, or for the admin the tenant's tree under
    // `universe`, read as `<user>-admin` (ADR-0097 d29).
    let (credential, root) = match side {
        Side::Personal => (session.credential(Capacity::Web), user.clone()),
        Side::Admin => (session.credential(Capacity::Admin), "universe".to_owned()),
    };
    let tree = match crate::pages::workspaces(web, &credential, calls, &root).await {
        Ok(tree) => tree,
        Err((request_id, Some(e))) => {
            return outcome::page(&e, calls, &format!("{}?{request_id}", side.path()));
        }
        Err(_) => {
            return refused(
                StatusCode::BAD_GATEWAY,
                "unavailable",
                "the workspace tree could not be read whole; try again",
                calls,
            );
        }
    };
    let review = pairs.iter().any(|(n, v)| n == "step" && v == "review");
    if review {
        let checks = draft.checks_as(side, &user, &tree, unix_now());
        if checks.is_empty() {
            let refused = pairs
                .iter()
                .find(|(n, _)| n == "refused")
                .map(|(_, v)| v.as_str());
            return draw(
                "Review key",
                &review_body_as(side, &draft, &user, &tree, &session.csrf(), refused),
            );
        }
        return draw(
            "New key",
            &form_body_as(side, &draft, &user, &tree, &checks, shape.is_some(), false),
        );
    }
    let root_shape = side == Side::Personal && shape == Some("root");
    draw(
        "New key",
        &form_body_as(side, &draft, &user, &tree, &[], root_shape, root_shape),
    )
}

/// Step one: the templates as links.
fn templates(side: Side, user: &str) -> String {
    let lead = match side {
        Side::Personal => format!("A key acting for <code>{}</code>.", short(user)),
        // The admin's issuance is widening: it waits on a second admin
        // person outside a one-admin tenant (ADR-0097 d5, d29).
        Side::Admin => "A key anywhere in the tenant. Its issuance waits on another admin's confirmation, except in a tenant that has only ever had one admin.".to_owned(),
    };
    let mut out = format!(
        r#"<div class="oml-col"><header class="oml-section-head"><div class="top"><h1>New key</h1></div><p class="desc">{lead} Start from a shape; every field can be changed on the next step, and nothing is issued before the review.</p></header><ol class="oml-steps"><li aria-current="step">Start from</li><li>Shape</li><li>Review</li></ol>"#
    );
    let personal: Vec<(&str, &str, &str)> = TEMPLATES
        .iter()
        .copied()
        .chain(std::iter::once((
            "new",
            "With a new workspace",
            "Creates a workspace inside the root in the same act and writes and recalls there.",
        )))
        .collect();
    let shapes: &[(&str, &str, &str)] = match side {
        Side::Personal => &personal,
        Side::Admin => &ADMIN_TEMPLATES,
    };
    let path = side.path();
    for (shape, title, desc) in shapes {
        let _ = write!(
            out,
            r#"<a class="oml-choice" href="{path}?shape={shape}"><strong>{}</strong><span class="desc">{}</span></a>"#,
            text(title),
            text(desc)
        );
    }
    let _ = write!(
        out,
        r#"<p><a href="{}">Back to Keys</a></p></div>"#,
        side.keys_path()
    );
    out
}

/// The workspaces of the root as `<option>`s, `chosen` selected.
fn options(tree: &[WorkspaceNode], chosen: &[String], none: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(none) = none {
        let _ = write!(out, r#"<option value="">{}</option>"#, text(none));
    }
    for node in tree {
        let _ = write!(
            out,
            r#"<option value="{}"{}>{}</option>"#,
            attr(&node.name),
            if chosen.contains(&node.name) {
                " selected"
            } else {
                ""
            },
            text(&node.name)
        );
    }
    out
}

fn option(value: &str, shown: &str, chosen: &str) -> String {
    format!(
        r#"<option value="{value}"{}>{}</option>"#,
        if value == chosen { " selected" } else { "" },
        text(shown)
    )
}

/// Step two: the form, with the error summary and each field's error where
/// `checks` names them.
#[cfg(test)]
pub(crate) fn form_body(
    draft: &Draft,
    user: &str,
    tree: &[WorkspaceNode],
    checks: &[Check],
    focus_name: bool,
    fold_grants: bool,
) -> String {
    form_body_as(
        Side::Personal,
        draft,
        user,
        tree,
        checks,
        focus_name,
        fold_grants,
    )
}

/// [`form_body`] on `side`'s builder: the admin's names the key whole and
/// adds the person, `used_by`, principal, crew and reason (ADR-0097 d29).
pub(crate) fn form_body_as(
    side: Side,
    draft: &Draft,
    user: &str,
    tree: &[WorkspaceNode],
    checks: &[Check],
    focus_name: bool,
    fold_grants: bool,
) -> String {
    let path = side.path();
    let mut out = String::from(
        r#"<div class="oml-col"><header class="oml-section-head"><div class="top"><h1>New key</h1></div></header><ol class="oml-steps"><li>Start from</li><li aria-current="step">Shape</li><li>Review</li></ol>"#,
    );
    if !checks.is_empty() {
        out.push_str(r#"<div class="oml-notice err" role="alert" tabindex="-1" id="errors"><p>The key is not issued: fix these first.</p><ul>"#);
        for c in checks {
            let _ = write!(
                out,
                r##"<li><a href="#{}">{}</a>{}</li>"##,
                attr(&c.control),
                text(&c.message),
                if c.code.is_empty() {
                    String::new()
                } else {
                    format!(" <code>{}</code>", c.code)
                }
            );
        }
        out.push_str("</ul></div>");
    }
    let err = |control: &str| checks.iter().any(|c| c.control == control);
    let invalid = |control: &str| {
        if err(control) {
            r#" aria-invalid="true""#
        } else {
            ""
        }
    };
    let _ = write!(
        out,
        r#"<form class="oml-form" method="get" action="{path}"><fieldset class="oml-radios"><legend>Kind</legend><label class="oml-check"><input type="radio" name="kind" value="agent"{}> Agent key: a value, shown once</label><label class="oml-check"><input type="radio" name="kind" value="connector"{}> Connector key: a connect code, shown once</label></fieldset>"#,
        if draft.connector { "" } else { " checked" },
        if draft.connector { " checked" } else { "" },
    );
    let prefix = match side {
        Side::Personal => format!(r#"<span class="p">{}-</span>"#, short(user)),
        Side::Admin => String::new(),
    };
    let _ = write!(
        out,
        r#"<label for="kn">Name</label><div class="oml-prefix">{prefix}<input class="oml-input" id="kn" name="name" value="{}" required{}{}></div>"#,
        attr(&draft.name),
        if focus_name { " autofocus" } else { "" },
        invalid("kn")
    );
    let reviewer = match side {
        Side::Personal => String::new(),
        Side::Admin => option("reviewer", "reviewer: review held memories", &draft.role),
    };
    let _ = write!(
        out,
        r#"<label>Role<select class="oml-select" id="kro" name="role"{}>{}{}{}{reviewer}</select></label>"#,
        invalid("kro"),
        option("contributor", "contributor: recall and write", &draft.role),
        option("reader", "reader: recall only", &draft.role),
        option("submitter", "submitter: write without recall", &draft.role),
    );
    if side == Side::Admin {
        let a = &draft.admin;
        let _ = write!(
            out,
            r#"<div class="oml-filterrow top"><label>Acts for the user<input class="oml-input" name="ku" value="{}"><span class="desc">Empty: a tenant key that names no user. With a user, every grant lies in that user's root.</span></label><label>Held by<select class="oml-select" name="kb">{}{}{}</select></label></div><div class="oml-filterrow top"><label>Principal<input class="oml-input" name="kp" value="{}"><span class="desc">Empty: the key's name.</span></label><label>Crew<input class="oml-input" name="kc" value="{}"><span class="desc">Empty: no crew. Fixed for the key's life.</span></label></div>"#,
            attr(&a.user),
            option("agent", "an agent", a.used_by()),
            option("person", "a person", &a.used_by),
            option("service", "a service", &a.used_by),
            attr(&a.principal),
            attr(&a.crew),
        );
    }
    // The Whole root template's grants are folded: the template is the
    // key the console issued before v0.8.5, and its grants change only
    // when asked (ADR-0096 d1).
    if fold_grants {
        let _ = write!(
            out,
            r#"<h2 id="gr-h" tabindex="-1">Grants</h2><ul class="desc">{}</ul><details class="oml-fold"><summary>Change grants</summary>"#,
            draft
                .words()
                .iter()
                .map(|w| format!("<li>{w}</li>"))
                .collect::<String>()
        );
    } else {
        out.push_str(r#"<h2 id="gr-h" tabindex="-1">Grants</h2>"#);
    }
    out.push_str(r#"<p class="desc">For a contributor, a submitter grant comes with a reader grant on the same workspace, as the command line issues it; remove the reader to write without recall there.</p>"#);
    for (i, g) in draft.grants.iter().enumerate() {
        let reviewer = match side {
            Side::Personal => String::new(),
            Side::Admin => option("reviewer", "reviewer", &g.role),
        };
        // The create box: the key creates child workspaces where this grant
        // reaches; granting it waits on another admin as the key does
        // (ADR-0097 d31).
        let _ = write!(
            out,
            r#"<fieldset class="oml-radios oml-grant{}"><legend>Grant {}</legend><div class="oml-filterrow top"><label>Workspace<select class="oml-select" id="g{i}w" name="g{i}w"{}>{}</select></label><label>Grant<select class="oml-select" name="g{i}r">{}{}{reviewer}</select></label><label>Covers<select class="oml-select" name="g{i}c">{}{}</select></label></div><label>Except<select class="oml-select" id="g{i}d" name="g{i}d" multiple{}>{}</select></label><label class="oml-check"><input type="checkbox" name="g{i}k" value="1"{}> Creates child workspaces here</label><label class="oml-check"><input type="checkbox" name="x{i}" value="1"> Remove this grant</label></fieldset>"#,
            if err(&format!("g{i}w")) || err(&format!("g{i}d")) {
                " is-invalid"
            } else {
                ""
            },
            i + 1,
            invalid(&format!("g{i}w")),
            options(tree, std::slice::from_ref(&g.workspace), Some("choose")),
            option("reader", "reader", &g.role),
            option("submitter", "submitter", &g.role),
            option("subtree", "this workspace and below", &g.covers),
            option("node", "this workspace only", &g.covers),
            invalid(&format!("g{i}d")),
            options(tree, &g.deny, None),
            if g.create { " checked" } else { "" },
        );
        // Grant counts carry on through the redraw.
    }
    let _ = write!(
        out,
        r#"<input type="hidden" name="n" value="{}"><p><button type="submit" name="add" value="1">Add a grant</button></p>"#,
        draft.grants.len()
    );
    if fold_grants {
        out.push_str("</details>");
    }
    // The "new workspace" rows: the issuance creates each in the same act
    // and grants the key its role's default grants there (ADR-0097 d30).
    out.push_str(r#"<h2 id="nw-h" tabindex="-1">New workspaces</h2><p class="desc">Created with the key, in one act, and granted to it by its role over the subtree. While the key waits on a confirmation, nothing is created.</p>"#);
    for (i, w) in draft.new_workspaces.iter().enumerate() {
        let _ = write!(
            out,
            r#"<fieldset class="oml-radios oml-grant{}"><legend>New workspace {}</legend><div class="oml-filterrow top"><label>Inside<select class="oml-select" id="w{i}p" name="w{i}p"{}>{}</select></label><label>Name<input class="oml-input" id="w{i}n" name="w{i}n" value="{}"{}></label></div><label class="oml-check"><input type="checkbox" name="y{i}" value="1"> Remove this workspace</label></fieldset>"#,
            if err(&format!("w{i}p")) || err(&format!("w{i}n")) {
                " is-invalid"
            } else {
                ""
            },
            i + 1,
            invalid(&format!("w{i}p")),
            options(tree, std::slice::from_ref(&w.parent), Some("choose")),
            attr(&w.name),
            invalid(&format!("w{i}n")),
        );
    }
    let _ = write!(
        out,
        r#"<input type="hidden" name="m" value="{}"><p><button type="submit" name="addw" value="1">Add a new workspace</button></p>"#,
        draft.new_workspaces.len()
    );
    let _ = write!(
        out,
        r#"<label>Default workspace<select class="oml-select" id="kd" name="dw"{}>{}</select></label>"#,
        invalid("kd"),
        options(
            tree,
            std::slice::from_ref(&draft.default_workspace),
            Some("the first grant's")
        )
    );
    let _ = write!(
        out,
        r#"<label>Guardrails loaded<select class="oml-select" name="gs">{}{}</select></label>"#,
        option(
            "ancestors",
            "the workspace's and every ancestor's the key reads",
            &draft.scope
        ),
        option("workspace", "the workspace's own", &draft.scope),
    );
    let sent = expiry_sent(&draft.expires).map_or_else(
        || "Empty: the key does not expire.".to_owned(),
        |at| {
            format!(
                "{}: works through it in every time zone; sent as {} {} UTC.",
                draft.expires,
                &at[..10],
                &at[11..19]
            )
        },
    );
    let account =
        "Empty: no rate of its own. Under the account's quota, the lower of the two holds.";
    let _ = write!(
        out,
        r#"<div class="oml-filterrow top"><label>Expires<input class="oml-input" type="date" id="ke" name="exp" value="{}"{}><span class="desc">{}</span></label><label>Requests a minute<input class="oml-input" id="kr" name="rpm" inputmode="numeric" value="{}"{}><span class="desc">{}</span></label></div><p class="desc">The key works through this date in every time zone. Only an admin changes the date or the rate after issue.</p>"#,
        attr(&draft.expires),
        invalid("ke"),
        text(&sent),
        attr(&draft.rate),
        invalid("kr"),
        text(account),
    );
    // Every widening act carries a reason, which the confirming admin
    // reads (ADR-0097 d26).
    if side == Side::Admin {
        let _ = write!(
            out,
            r#"<label for="why">Reason</label><textarea class="oml-input" id="why" name="why" rows="2" required{}>{}</textarea><p class="desc">Shown to the admin who confirms the key, and kept with the act.</p>"#,
            invalid("why"),
            text(&draft.admin.reason),
        );
    }
    let _ = write!(
        out,
        r#"<p><button class="primary" type="submit" name="step" value="review">Review</button> <a href="{}">Cancel</a></p></form></div>"#,
        side.keys_path()
    );
    out
}

/// Step three: the review, read-only, with **Issue key** the one post.
#[cfg(test)]
pub(crate) fn review_body(
    draft: &Draft,
    user: &str,
    tree: &[WorkspaceNode],
    csrf: &str,
    refused: Option<&str>,
) -> String {
    review_body_as(Side::Personal, draft, user, tree, csrf, refused)
}

/// [`review_body`] on `side`'s builder: the admin's posts to the admin
/// Keys page and says the key waits on another admin (ADR-0097 d29).
pub(crate) fn review_body_as(
    side: Side,
    draft: &Draft,
    user: &str,
    tree: &[WorkspaceNode],
    csrf: &str,
    refused: Option<&str>,
) -> String {
    let shown_name = match side {
        Side::Personal => format!("{}-{}", short(user), short(&draft.name)),
        Side::Admin => short(&draft.name),
    };
    let mut out = format!(
        r#"<div class="oml-col"><header class="oml-section-head"><div class="top"><h1>Review key</h1></div><p class="desc"><code>{shown_name}</code>, a {} {}.</p></header><ol class="oml-steps"><li>Start from</li><li>Shape</li><li aria-current="step">Review</li></ol>"#,
        text(&draft.role),
        if draft.connector {
            "connector key"
        } else {
            "agent key"
        }
    );
    if let Some((code, sentence)) = refused.and_then(server_refusal) {
        let _ = write!(
            out,
            r#"<div class="oml-notice err" role="alert" tabindex="-1" id="errors"><p>The key is not issued: {sentence}</p><p><code>{code}</code></p></div>"#
        );
    }
    out.push_str(r#"<section class="oml-panel"><h2 class="oml-h3">Effective access</h2><ul>"#);
    for line in draft.words() {
        let _ = write!(out, "<li>{line}</li>");
    }
    out.push_str("</ul>");
    let guardrails = if draft.role == "submitter" {
        "Guardrails loaded: none, since the key reads nothing.".to_owned()
    } else if draft.scope == "workspace" {
        "Guardrails loaded: the workspace's own.".to_owned()
    } else {
        "Guardrails loaded: the workspace's and every ancestor's the key reads; a workspace it does not read loads none.".to_owned()
    };
    let _ = write!(out, r#"<p class="desc">{}</p>"#, text(&guardrails));
    let mut facts = Vec::new();
    if !draft.expires.is_empty() {
        facts.push(format!(
            "works through {} in every time zone",
            text(&draft.expires)
        ));
    }
    if !draft.rate.is_empty() {
        facts.push(format!("{} requests a minute", text(&draft.rate)));
    }
    if !facts.is_empty() {
        let _ = write!(out, r#"<p class="desc">{}</p>"#, facts.join(" · "));
    }
    out.push_str(r#"<ul class="oml-tree">"#);
    for node in tree {
        let depth = node.path.matches('.').count();
        let marks = draft.marks(node, tree);
        let off = if marks.contains(&"not covered") {
            " off"
        } else {
            ""
        };
        let _ = write!(
            out,
            r#"<li class="n d{}{off}"><span class="name">{}</span> {}</li>"#,
            depth.min(6),
            text(&node.name),
            marks
                .iter()
                .map(|m| format!(r#"<span class="oml-tag">{m}</span>"#))
                .collect::<String>()
        );
    }
    out.push_str("</ul></section>");
    let body = draft.body_as(side, user);
    let _ = write!(
        out,
        r#"<details><summary>The exact request</summary><pre>POST /v2/keys
{}</pre></details>"#,
        text(&serde_json::to_string_pretty(&body).unwrap_or_default())
    );
    if draft.role == "submitter" {
        out.push_str(r#"<p class="oml-consequence">A submitter key imports nothing and creates no workspace. As a connector, the connection writes and recalls nothing.</p>"#);
    }
    for workspace in draft.without_recall(tree) {
        let _ = write!(
            out,
            r#"<p class="oml-consequence">No recall: the key writes in <code>{}</code> without recall there, and is listed "no recall".</p>"#,
            short(&workspace)
        );
    }
    let _ = write!(
        out,
        r#"<p class="oml-consequence">The {} is shown once, on the next page. Only an admin changes its date or rate afterwards.</p>"#,
        if draft.connector {
            "connect code"
        } else {
            "value"
        }
    );
    if side == Side::Admin {
        let a = &draft.admin;
        let mut facts = vec![format!("held by {}", text(a.used_by()))];
        if !a.user.is_empty() {
            facts.push(format!("acts for <code>{}</code>", short(&a.user)));
        }
        if !a.crew.is_empty() {
            facts.push(format!("crew <code>{}</code>", short(&a.crew)));
        }
        let _ = write!(
            out,
            r#"<p class="desc">{}</p><p class="desc">Reason: {}</p><p class="oml-consequence">Waits on another admin: the key works nowhere, and no workspace is created, until a second admin confirms it, except in a tenant that has only ever had one admin, where it takes effect at once, marked.</p>"#,
            facts.join(" · "),
            text(&a.reason)
        );
    }
    let mut back: Vec<String> = draft
        .fields()
        .into_iter()
        .map(|(n, v)| format!("{n}={}", oml_sdk::encode(&v)))
        .collect();
    back.retain(|p| !p.is_empty());
    let _ = write!(
        out,
        r#"<form method="post" action="{}"><input type="hidden" name="action" value="build_key"><input type="hidden" name="csrf_token" value="{}">{}{}<button class="primary" type="submit">Issue key</button></form><p><a href="{}?{}">Change</a></p></div>"#,
        side.keys_path(),
        attr(csrf),
        draft.hidden(),
        acts::nonce_field(),
        side.path(),
        attr(&back.join("&"))
    );
    out
}

/// The refusals only the server decides (D12), each with its sentence,
/// that bring **Issue key** back to the review step with the draft kept:
/// a closed set, so a crafted link cannot put text on the page.
pub(crate) fn server_refusal(code: &str) -> Option<(&'static str, &'static str)> {
    match code {
        "name.taken" => Some((
            "name.taken",
            "the name is taken by another key. Change it and review again.",
        )),
        "limit.quota" => Some((
            "limit.quota",
            "the account holds as many keys as its quota admits. Cut off a key first, or ask an admin to raise the quota.",
        )),
        "oauth.connect_refused" => Some((
            "oauth.connect_refused",
            "the server issues no connector key of this shape here. Change the kind or the role and review again.",
        )),
        _ => None,
    }
}

/// The draft a `build_key` post carries: each Except list one field, its
/// workspaces joined by commas.
pub(crate) fn of_form(fields: &Fields) -> Result<Draft, String> {
    let mut pairs = Vec::new();
    for name in fields.names() {
        if matches!(name, "action" | "csrf_token" | "nonce") {
            continue;
        }
        let value = fields.get(name).unwrap_or_default();
        if grant_index(name, 'd').is_some() {
            for d in value.split(',').filter(|d| !d.is_empty()) {
                pairs.push((name.to_owned(), d.to_owned()));
            }
        } else {
            pairs.push((name.to_owned(), value.to_owned()));
        }
    }
    Draft::of_query(&pairs)
}

/// Whether `name` is a field of a `build_key` post.
pub(crate) fn defined(name: &str) -> bool {
    matches!(
        name,
        "kind" | "name" | "role" | "n" | "m" | "dw" | "gs" | "exp" | "rpm"
    ) || ['w', 'r', 'c', 'd', 'k']
        .iter()
        .any(|s| grant_index(name, *s).is_some_and(|i| i <= GRANTS_MAX))
        || ['p', 'n']
            .iter()
            .any(|s| new_index(name, *s).is_some_and(|i| i <= GRANTS_MAX))
}

/// Whether `name` is a field of the admin builder's `build_key` post: the
/// personal ones and the admin's own (ADR-0097 d29).
pub(crate) fn defined_admin(name: &str) -> bool {
    defined(name) || matches!(name, "ku" | "kb" | "kp" | "kc" | "why")
}

/// `w<i><suffix>`'s index, a "new workspace" row's field.
fn new_index(name: &str, suffix: char) -> Option<usize> {
    name.strip_prefix('w')?.strip_suffix(suffix)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, path: &str) -> WorkspaceNode {
        serde_json::from_value(json!({"id": name, "name": name, "path": path})).unwrap()
    }

    fn tree() -> Vec<WorkspaceNode> {
        vec![
            node("alice", "universe.alice"),
            node("alice.proj", "universe.alice.proj"),
            node("alice.notes", "universe.alice.notes"),
        ]
    }

    #[test]
    fn the_expiry_date_is_sent_as_the_end_of_the_next_utc_day() {
        assert_eq!(
            expiry_sent("2026-12-01").as_deref(),
            Some("2026-12-02T23:59:59Z")
        );
        assert_eq!(
            expiry_sent("2026-12-31").as_deref(),
            Some("2027-01-01T23:59:59Z")
        );
        // A day its month does not hold rolls over.
        assert_eq!(
            expiry_sent("2026-02-30").as_deref(),
            Some("2026-03-03T23:59:59Z")
        );
        assert_eq!(expiry_sent("2026-12-1"), None);
        assert_eq!(expiry_sent("tomorrow"), None);
        let draft = Draft {
            expires: "2026-12-01".to_owned(),
            ..Draft::template("root", "alice")
        };
        assert_eq!(draft.body("alice")["expiry"], "2026-12-02T23:59:59Z");
    }

    #[test]
    fn a_whole_root_template_sends_a_submitter_and_a_reader_grant_on_the_root() {
        let draft = Draft {
            name: "laptop".to_owned(),
            ..Draft::template("root", "alice")
        };
        assert!(draft.checks("alice", &tree(), 0).is_empty());
        let body = draft.body("alice");
        assert_eq!(body["name"], "alice-laptop");
        assert_eq!(body["role"], "contributor");
        assert_eq!(body["grants"].as_array().unwrap().len(), 2);
        assert!(body.get("requests_per_minute").is_none());
    }

    #[test]
    fn the_review_refuses_what_the_server_would_refuse_naming_each_control() {
        let codes = |draft: &Draft| -> Vec<(&'static str, String)> {
            draft
                .checks("alice", &tree(), 0)
                .into_iter()
                .map(|c| (c.code, c.control))
                .collect()
        };
        let mut draft = Draft {
            name: "review".to_owned(),
            ..Draft::template("root", "alice")
        };
        assert_eq!(codes(&draft), [("request.validation", "kn".to_owned())]);
        draft.name = "proj-review".to_owned();
        assert!(codes(&draft).is_empty());
        draft.grants[0].workspace = "universe".to_owned();
        assert!(codes(&draft).contains(&("access.issuer_ceiling", "g0w".to_owned())));
        let mut reader = Draft {
            name: "r".to_owned(),
            ..Draft::template("read", "alice")
        };
        reader.grants[0].role = "submitter".to_owned();
        assert!(codes(&reader).contains(&("access.issuer_ceiling", "g0w".to_owned())));
        let mut node = Draft {
            name: "n".to_owned(),
            ..Draft::template("root", "alice")
        };
        node.grants[1].covers = "node".to_owned();
        node.grants[1].deny = vec!["alice.proj".to_owned()];
        assert!(codes(&node).contains(&("request.validation", "g1d".to_owned())));
        node.grants[1].covers = "subtree".to_owned();
        node.default_workspace = "alice.proj".to_owned();
        node.grants[0].deny = vec!["alice.proj".to_owned()];
        assert!(codes(&node).contains(&("request.validation", "kd".to_owned())));
        let mut rate = Draft {
            name: "x".to_owned(),
            rate: "0".to_owned(),
            ..Draft::template("root", "alice")
        };
        assert_eq!(codes(&rate), [("request.validation", "kr".to_owned())]);
        rate.rate = "1000000".to_owned();
        assert!(codes(&rate).is_empty());
        let only_writes = Draft {
            name: "w".to_owned(),
            grants: vec![Draft::template("root", "alice").grants[0].clone()],
            ..Draft::template("root", "alice")
        };
        assert!(codes(&only_writes).contains(&("request.validation", "gr-h".to_owned())));
    }

    #[test]
    fn a_contributor_whose_submitter_grant_no_reader_reaches_is_warned_no_recall() {
        let mut draft = Draft {
            name: "w".to_owned(),
            ..Draft::template("one", "alice")
        };
        draft.grants[0].workspace = "alice.notes".to_owned();
        draft.grants[1].workspace = "alice.proj".to_owned();
        assert_eq!(draft.without_recall(&tree()), ["alice.notes"]);
        let page = review_body(&draft, "alice", &tree(), "csrf", None);
        assert!(page.contains("No recall"), "{page}");
        assert!(page.contains(r#"value="build_key""#), "{page}");
    }

    #[test]
    fn a_submitter_key_reads_nothing_in_universe() {
        let draft = Draft {
            name: "ingest".to_owned(),
            ..Draft::template("write", "alice")
        };
        let words = draft.words().join(" ");
        assert!(words.contains("Reads nothing in"), "{words}");
        assert!(draft.checks("alice", &tree(), 0).is_empty());
    }

    #[test]
    fn a_date_over_in_every_time_zone_is_the_pages_own_check() {
        let now = 1_794_392_400; // 2026-11-11T10:20:00Z
        let draft = |exp: &str| Draft {
            name: "x".to_owned(),
            expires: exp.to_owned(),
            ..Draft::template("root", "alice")
        };
        assert!(draft("2026-11-10").checks("alice", &tree(), now).is_empty());
        assert_eq!(draft("2026-11-09").checks("alice", &tree(), now).len(), 1);
    }

    #[test]
    fn a_date_over_in_every_time_zone_is_drawn_with_no_code() {
        let now = 1_794_392_400; // 2026-11-11T10:20:00Z
        let draft = Draft {
            name: "x".to_owned(),
            expires: "2026-11-09".to_owned(),
            ..Draft::template("root", "alice")
        };
        let checks = draft.checks("alice", &tree(), now);
        let page = form_body(&draft, "alice", &tree(), &checks, false, false);
        assert!(
            page.contains("2026-11-09 is over in every time zone."),
            "{page}"
        );
        assert!(!page.contains("</a> <code>"), "{page}");
    }

    #[test]
    fn the_whole_root_template_folds_its_grants_under_change_grants() {
        let draft = Draft::template("root", "alice");
        let folded = form_body(&draft, "alice", &tree(), &[], true, true);
        assert!(
            folded.contains(r#"<details class="oml-fold"><summary>Change grants</summary>"#),
            "{folded}"
        );
        assert!(folded.contains("Grant 1"), "{folded}");
        let open = form_body(&draft, "alice", &tree(), &[], false, false);
        assert!(!open.contains("Change grants"), "{open}");
        assert!(open.contains("Grant 1"), "{open}");
    }

    #[test]
    fn the_submitter_warning_is_on_the_review_step() {
        let draft = Draft {
            name: "ingest".to_owned(),
            ..Draft::template("write", "alice")
        };
        let warning = "A submitter key imports nothing and creates no workspace.";
        let form = form_body(&draft, "alice", &tree(), &[], false, false);
        assert!(!form.contains(warning), "{form}");
        let review = review_body(&draft, "alice", &tree(), "csrf", None);
        assert!(review.contains(warning), "{review}");
    }

    #[test]
    fn a_node_one_grant_excepts_and_another_reaches_is_marked_except() {
        let mut draft = Draft {
            name: "x".to_owned(),
            ..Draft::template("root", "alice")
        };
        draft.grants[0].deny = vec!["alice.proj".to_owned()];
        let tree = tree();
        assert_eq!(draft.marks(&tree[1], &tree), ["recall", "except"]);
        let review = review_body(&draft, "alice", &tree, "csrf", None);
        assert!(
            review.contains(
                r#"<span class="oml-tag">recall</span><span class="oml-tag">except</span>"#
            ),
            "{review}"
        );
    }

    #[test]
    fn a_server_only_refusal_is_drawn_on_the_review_step_with_its_code() {
        let draft = Draft {
            name: "ci".to_owned(),
            ..Draft::template("root", "alice")
        };
        for code in ["name.taken", "limit.quota", "oauth.connect_refused"] {
            let review = review_body(&draft, "alice", &tree(), "csrf", Some(code));
            assert!(review.contains(&format!("<code>{code}</code>")), "{review}");
            assert!(review.contains(r#"role="alert""#), "{review}");
            assert!(review.contains(r#"value="build_key""#), "{review}");
        }
        let crafted = review_body(&draft, "alice", &tree(), "csrf", Some("<b>x</b>"));
        assert!(!crafted.contains(r#"role="alert""#), "{crafted}");
    }

    #[test]
    fn a_draft_reads_back_from_its_own_fields_and_its_post() {
        let mut draft = Draft {
            name: "bot".to_owned(),
            rate: "60".to_owned(),
            ..Draft::template("root", "alice")
        };
        draft.grants[1].deny = vec!["alice.proj".to_owned(), "alice.notes".to_owned()];
        assert_eq!(Draft::of_query(&draft.fields()).unwrap(), draft);
        let posted: Vec<(String, String)> = {
            let html = draft.hidden();
            html.split("<input type=\"hidden\" name=\"")
                .skip(1)
                .map(|part| {
                    let (name, rest) = part.split_once('"').unwrap();
                    let value = rest.split_once("value=\"").unwrap().1;
                    (
                        name.to_owned(),
                        value[..value.find('"').unwrap()].to_owned(),
                    )
                })
                .collect()
        };
        let fields = Fields::of_pairs(posted).unwrap();
        assert_eq!(of_form(&fields).unwrap(), draft);
    }

    #[test]
    fn a_new_workspace_row_and_a_create_box_reach_the_request_and_read_back() {
        let mut draft = Draft {
            name: "bot".to_owned(),
            ..Draft::template("new", "alice")
        };
        draft.new_workspaces[0].name = "proj2".to_owned();
        draft.grants.push(GrantDraft {
            workspace: "alice.proj".to_owned(),
            role: "submitter".to_owned(),
            covers: "node".to_owned(),
            create: true,
            ..GrantDraft::default()
        });
        assert!(
            draft.checks("alice", &tree(), 0).is_empty(),
            "{:?}",
            draft.checks("alice", &tree(), 0)
        );
        let body = draft.body("alice");
        assert_eq!(body["create_workspaces"][0]["parent"], "alice");
        assert_eq!(body["create_workspaces"][0]["name"], "proj2");
        assert_eq!(body["grants"][0]["create"], true);
        assert_eq!(Draft::of_query(&draft.fields()).unwrap(), draft);
        assert!(
            draft
                .words()
                .iter()
                .any(|w| w.contains("creates workspaces there"))
        );
    }

    #[test]
    fn the_admin_builder_sends_the_name_whole_and_requires_a_reason() {
        let mut draft = Draft {
            name: "ops-bot".to_owned(),
            ..Draft::template("review", "alice")
        };
        let checks = draft.checks_as(Side::Admin, "", &tree(), 0);
        assert_eq!(
            checks
                .iter()
                .map(|c| c.control.as_str())
                .collect::<Vec<_>>(),
            ["why"]
        );
        draft.admin.reason = "triage".to_owned();
        draft.admin.crew = "ops".to_owned();
        assert!(draft.checks_as(Side::Admin, "", &tree(), 0).is_empty());
        let body = draft.body_as(Side::Admin, "");
        assert_eq!(body["name"], "ops-bot");
        assert_eq!(body["role"], "reviewer");
        assert_eq!(body["principal"], "ops-bot");
        assert_eq!(body["reason"], "triage");
        assert_eq!(body["crew"], "ops");
        assert_eq!(body["used_by"], "agent");
        // A person's own builder takes no reviewer key.
        assert!(!draft.checks("alice", &tree(), 0).is_empty());
    }
}
