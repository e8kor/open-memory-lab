//! A memory version's `origin.kind`: what kind of source its text was
//! taken from, beside the anchor that says which one (ADR-0063 d1-d3).
//!
//! The kind is the author's statement, signed in the submission event and
//! immutable per version. This module holds the closed set, the request
//! object's grammar and the two compositions with the anchor; the server
//! never infers a kind.

use serde_json::{Map, Value};

use crate::anchor::{Anchor, AnchorError};

/// The closed set of kinds (ADR-0063 d1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OriginKind {
    /// A repository's metadata: a commit, a pull request, an issue.
    Repo,
    /// A document's prose.
    Doc,
    /// Source code.
    Code,
    /// A lake table's rows or schema.
    Lake,
    /// A conversation with a person.
    Chat,
    /// A page fetched over https.
    Web,
    /// Written from no source the author names; the default.
    Manual,
}

impl OriginKind {
    /// Every kind, in the order the contract lists them.
    pub const ALL: [Self; 7] = [
        Self::Repo,
        Self::Doc,
        Self::Code,
        Self::Lake,
        Self::Chat,
        Self::Web,
        Self::Manual,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Doc => "doc",
            Self::Code => "code",
            Self::Lake => "lake",
            Self::Chat => "chat",
            Self::Web => "web",
            Self::Manual => "manual",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// `origin` as served and journalled: `{"kind": …}`.
    #[must_use]
    pub fn to_json(self) -> Value {
        let mut object = Map::new();
        object.insert("kind".into(), Value::from(self.as_str()));
        Value::Object(object)
    }

    /// Parses the request's `origin` object under `base` (`/origin`): a
    /// closed object whose one member, `kind`, is required and one of the
    /// closed set (ADR-0063 d1).
    ///
    /// # Errors
    ///
    /// [`AnchorError`] naming `/origin` or `/origin/<member>`.
    pub fn from_json(value: &Value, base: &str) -> Result<Self, AnchorError> {
        let refuse = |field: String, reason: &str| AnchorError {
            field,
            reason: reason.to_owned(),
        };
        let Some(object) = value.as_object() else {
            return Err(refuse(base.to_owned(), "origin is an object"));
        };
        if let Some(name) = object.keys().find(|k| k.as_str() != "kind") {
            return Err(refuse(format!("{base}/{name}"), "not a member of origin"));
        }
        let kind = object
            .get("kind")
            .ok_or_else(|| refuse(format!("{base}/kind"), "required"))?;
        kind.as_str().and_then(Self::parse).ok_or_else(|| {
            refuse(
                format!("{base}/kind"),
                "one of repo, doc, code, lake, chat, web, manual",
            )
        })
    }

    /// The two compositions with the anchor (ADR-0063 d2): a chat anchor
    /// requires `chat`, and `chat` requires a chat anchor.
    ///
    /// # Errors
    ///
    /// [`AnchorError`] naming `/origin/kind`.
    pub fn check_with(self, anchor: Option<&Anchor>) -> Result<(), AnchorError> {
        let chat_anchor = anchor.is_some_and(Anchor::is_chat);
        match (self == Self::Chat, chat_anchor) {
            (true, false) => Err(AnchorError {
                field: "/origin/kind".to_owned(),
                reason: "chat requires a chat anchor, chat://<harness>/<session>#turn=<n>"
                    .to_owned(),
            }),
            (false, true) => Err(AnchorError {
                field: "/origin/kind".to_owned(),
                reason: "a chat anchor requires origin kind chat".to_owned(),
            }),
            _ => Ok(()),
        }
    }
}

/// The `origin_kinds` filter: one to seven distinct kinds (ADR-0063 d3).
///
/// # Errors
///
/// Why it fails: empty, more than seven, a repeat, or an unknown kind.
pub fn parse_kinds<'a>(
    values: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<OriginKind>, String> {
    let mut out: Vec<OriginKind> = Vec::new();
    for v in values {
        let kind = OriginKind::parse(v).ok_or_else(|| {
            format!("{v:?} is not one of repo, doc, code, lake, chat, web, manual")
        })?;
        if out.contains(&kind) {
            return Err(format!("{v:?} is named twice"));
        }
        out.push(kind);
    }
    if out.is_empty() {
        return Err("one to seven kinds".to_owned());
    }
    Ok(out)
}
