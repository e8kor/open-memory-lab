//! Workspace imports (ADR-0084 d6-d11, I4; #426): an import's states and
//! the outcome names its counts use. Pure: the bundle's checks and the plan
//! are `oml-bundle`'s, the app applies them, the store keeps the rows.

/// An import's state. An import runs after the request that binds its
/// upload answers (ADR-0084 I10); one `running` longer than
/// [`STOPPED_AFTER_MICROS`] that no process runs stopped mid-import, and
/// the sweep ends it `failed` (I11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportState {
    Running,
    Done,
    Failed,
}

impl ImportState {
    pub const ALL: [Self; 3] = [Self::Running, Self::Done, Self::Failed];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == s)
    }
}

/// How long an import may stay `running` before the sweep ends it
/// `failed`, unless this process runs it (ADR-0084 I11): one hour.
pub const STOPPED_AFTER_MICROS: i64 = 3_600 * 1_000_000;

/// What happened to one planned claim key, as the outcome counts it and
/// `oml_import_versions_total{outcome}` labels it: the plan's six outcomes
/// (d8), and `refused` for a version the target's scan or rules refused,
/// which is skipped and named (d11).
pub const OUTCOMES: [&str; 7] = [
    "new",
    "changed",
    "removed",
    "unchanged",
    "equivalent",
    "conflict",
    "refused",
];

/// Whether an outcome writes a version on the target (d8).
#[must_use]
pub fn writes(outcome: &str) -> bool {
    matches!(outcome, "new" | "changed" | "removed" | "conflict")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_round_trip_their_spelling() {
        for state in ImportState::ALL {
            assert_eq!(ImportState::parse(state.as_str()), Some(state));
        }
        assert_eq!(ImportState::parse("planned"), None);
    }

    #[test]
    fn only_new_changed_removed_and_conflict_write() {
        let written: Vec<&str> = OUTCOMES.into_iter().filter(|o| writes(o)).collect();
        assert_eq!(written, ["new", "changed", "removed", "conflict"]);
    }
}
