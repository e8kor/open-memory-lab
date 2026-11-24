//! Workspace export (ADR-0084 d2, d10; #425): the tenant's export policy and
//! the shape of a key that may export. Pure: the app applies them, the store
//! keeps the policy and the allowances.

use crate::{Role, UsedBy};

/// The tenant policy `export` (d10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportPolicy {
    /// No workspace leaves the server.
    Off,
    /// A person exports only inside their own root, unless an admin allowed
    /// one shared workspace's export. The default.
    #[default]
    OwnRoot,
    /// Shared workspace subtrees too.
    Allowed,
}

impl ExportPolicy {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::OwnRoot => "own_root",
            Self::Allowed => "allowed",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "off" => Some(Self::Off),
            "own_root" => Some(Self::OwnRoot),
            "allowed" => Some(Self::Allowed),
            _ => None,
        }
    }
}

/// Whether a key of this shape may export (d10): a key used by a person,
/// never an admin key, never one under a profile and never one that belongs
/// to a crew; no agent or service key exports (ADR-0084 readings I1, I18).
#[must_use]
pub fn may_export(role: Role, used_by: UsedBy, has_profile: bool, in_crew: bool) -> bool {
    used_by == UsedBy::Person && role != Role::Admin && !has_profile && !in_crew
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_export_policy_is_own_root() {
        assert_eq!(ExportPolicy::default(), ExportPolicy::OwnRoot);
        for p in [
            ExportPolicy::Off,
            ExportPolicy::OwnRoot,
            ExportPolicy::Allowed,
        ] {
            assert_eq!(ExportPolicy::parse(p.as_str()), Some(p));
        }
        assert_eq!(ExportPolicy::parse("on"), None);
    }

    #[test]
    fn only_a_persons_non_admin_key_may_export() {
        assert!(may_export(Role::Reader, UsedBy::Person, false, false));
        assert!(may_export(Role::Reviewer, UsedBy::Person, false, false));
        assert!(!may_export(Role::Admin, UsedBy::Person, false, false));
        assert!(!may_export(Role::Contributor, UsedBy::Agent, false, false));
        assert!(!may_export(Role::Reviewer, UsedBy::Person, true, false));
    }

    #[test]
    fn a_crew_key_never_exports() {
        // I18: a key that belongs to a crew is refused as an agent key is.
        assert!(!may_export(Role::Reader, UsedBy::Person, false, true));
        assert!(!may_export(Role::Contributor, UsedBy::Person, false, true));
    }
}
