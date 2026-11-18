//! Workspace imports (ADR-0084 d6-d11, I4; #426): an import's states and
//! the outcome names its counts use. Pure: the bundle's checks and the plan
//! are `oml-bundle`'s, the app applies them, the store keeps the rows.

/// An import's state. An import runs inline in the request that binds its
/// upload, so `running` outlives a request only when the process stopped
/// mid-import; the doctor counts those (d15).
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
