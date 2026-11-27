//! In-product notices (ADR-0097 d20): an act against an admin, offensive
//! narrowing (d24), a reach into a person's root under `sole_admin` (d6), a
//! recovery and a settlement (d19) each write a notice, in the act's own
//! commit, to the persons it concerns. The person reads it in To do. Only a
//! recovery and a settlement are also emailed, since a dormant admin sees
//! nothing in-product.

/// What a notice tells its recipients about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoticeKind {
    /// An act against an admin person (d8): every admin person.
    ActAgainstAdmin,
    /// Offensive narrowing (d24): every admin person and every reviewer
    /// person of the workspace concerned, and the person it acted on.
    OffensiveNarrowing,
    /// A reach into a person's own root taken at once under `sole_admin`
    /// (d6): the root's person.
    RootReach,
    /// A host recovery of a dormant tenant (d19): every admin person.
    Recovery,
    /// A host settlement of a frozen dispute (d19): both parties.
    Settlement,
}

/// Every kind, for the round-trip check.
pub const NOTICE_KINDS: &[NoticeKind] = &[
    NoticeKind::ActAgainstAdmin,
    NoticeKind::OffensiveNarrowing,
    NoticeKind::RootReach,
    NoticeKind::Recovery,
    NoticeKind::Settlement,
];

impl NoticeKind {
    /// The spelling of `notice.written.kind` and of the column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            NoticeKind::ActAgainstAdmin => "act_against_admin",
            NoticeKind::OffensiveNarrowing => "offensive_narrowing",
            NoticeKind::RootReach => "root_reach",
            NoticeKind::Recovery => "recovery",
            NoticeKind::Settlement => "settlement",
        }
    }

    /// The kind a spelling names; `None` for any other string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        NOTICE_KINDS.iter().copied().find(|k| k.as_str() == s)
    }

    /// Whether the notice is also emailed to every admin person: a recovery
    /// and a settlement only (d20).
    #[must_use]
    pub fn emailed(self) -> bool {
        matches!(self, NoticeKind::Recovery | NoticeKind::Settlement)
    }

    /// Whether doctor raises the notice's act as `critical` (d20, d24):
    /// all but a reach under `sole_admin`, which the act's own mark and
    /// doctor's `admin.act_sole_admin` already name.
    #[must_use]
    pub fn critical(self) -> bool {
        !matches!(self, NoticeKind::RootReach)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_notice_kind_round_trips_and_only_recovery_and_settlement_are_emailed() {
        for kind in NOTICE_KINDS {
            assert_eq!(NoticeKind::parse(kind.as_str()), Some(*kind));
            assert_eq!(
                kind.emailed(),
                matches!(kind, NoticeKind::Recovery | NoticeKind::Settlement)
            );
        }
    }
}
