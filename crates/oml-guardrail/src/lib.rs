//! Guardrail rules — the part of proposed ADR-0037 both the server and the
//! agent plugin's hooks read: the rule grammar `oml-gr/1` and its tool
//! classes (d8), the severity and effect it names (d2, d8), the linear-time
//! glob (d8), the check before a tool call (d8, d9) and the rule cache's
//! codec (d9), with the design constants they share (d7, d9).
//!
//! A leaf crate: it links no workspace crate, so `oml-domain`, which
//! re-exports it, and `oml-cli`, whose hooks run the check, cannot disagree
//! on what a rule means (d18 Q10, amending ADR-0006 d6 by this one leaf).
//! Serving, admission, the hooks' I/O and the clock are not here.

#![forbid(unsafe_code)]

mod argument;
mod cache;
mod canonical;
mod check;
mod glob;
mod rule;
mod shell;

pub use argument::{normalise_path, url_host};
pub use cache::{
    CacheRefusal, CacheState, CachedRule, SessionIdRefusal, cache_file_name, decode_cache,
    encode_cache,
};
pub use check::{Cause, Decision, NO_CACHE_WARNING, ToolCall, Uncheckable, Verdict, check, decide};
pub use glob::Glob;
pub use rule::{HostPattern, Matcher, Rule, RuleDefect, ToolClass};
pub use shell::{ShellRefusal, ShellSegment, shell_command, shell_segments};

/// The one rule grammar (ADR-0037 d8). A cache naming another denies every
/// call (d9).
pub const GRAMMAR: &str = "oml-gr/1";

/// Admission caps a guardrail's body at 300 UTF-8 bytes (ADR-0037 d7).
pub const BODY_MAX_BYTES: usize = 300;

/// Admission caps a rule at 160 bytes, measured on its RFC 8785 canonical
/// form (ADR-0037 d7; the form is this module's reading).
pub const RULE_MAX_BYTES: usize = 160;

/// The hook caps a line's provenance at 200 characters (ADR-0037 d7).
pub const PROVENANCE_MAX_CHARS: usize = 200;

/// The read lists at most five of the caller's own held guardrails
/// (ADR-0037 d3, d7).
pub const OWN_HELD_MAX_LINES: usize = 5;

/// At most 12 guardrails apply at a node; over it no partial set is printed
/// (ADR-0037 d7).
pub const SET_CAP: usize = 12;

/// Cache files older than 30 days are removed at a session start
/// (ADR-0037 d9).
pub const CACHE_MAX_AGE_DAYS: u32 = 30;

/// How binding a guardrail is (ADR-0037 d2): `never` and `must` are
/// binding, after RFC 2119's MUST NOT and MUST, and `should` is the
/// reviewed default. The declaration order is the set's first order key,
/// strictest first: `never`, `must`, `should` (d4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    Never,
    Must,
    Should,
}

impl Severity {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Never => "never",
            Severity::Must => "must",
            Severity::Should => "should",
        }
    }

    /// The severity an exact word spells; `None` for any other string,
    /// which a caller refuses rather than coerces.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "never" => Some(Severity::Never),
            "must" => Some(Severity::Must),
            "should" => Some(Severity::Should),
            _ => None,
        }
    }

    /// Whether a rule of this severity may take `effect`: `never` and
    /// `must` deny or ask, and `should` takes `ask` only (ADR-0037 d8).
    #[must_use]
    pub fn admits(self, effect: Effect) -> bool {
        !(self == Severity::Should && effect == Effect::Deny)
    }
}

/// A rule's effect, `deny` or `ask`; there is no `allow` (ADR-0037 d8). The
/// stricter effect is the greater, so the effect of several matching rules
/// is their maximum: deny wins over ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Effect {
    Ask,
    Deny,
}

impl Effect {
    /// The `permissionDecision` value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Effect::Ask => "ask",
            Effect::Deny => "deny",
        }
    }

    /// `deny` or `ask`; `None` for `allow` and every other string.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "ask" => Some(Effect::Ask),
            "deny" => Some(Effect::Deny),
            _ => None,
        }
    }
}
