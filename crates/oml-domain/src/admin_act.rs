//! The admin act (ADR-0097 d1, d2): every act an admin takes outside content
//! review is one record under one fail-closed rule, and the act decides its
//! class. Narrowing and neutral acts take effect at once; widening waits on a
//! second admin person who counts (d5, d11), except in a `sole_admin` tenant
//! (d10); an act against another admin takes effect at once and is
//! contestable (d8). The class is fixed per act here and never sent by a
//! caller, and no act sits in two classes: where the class depends on the
//! target, the act has two names (`revoke_key`, `revoke_admin_key`).

/// The six classes of ADR-0097 d2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AdminActClass {
    /// Takes effect at once and never reverts by itself (d3).
    Narrowing,
    /// Demotes at once; the shred waits on a second admin person (d4).
    Erase,
    /// Takes effect only after a second admin person confirms it (d5).
    Widening,
    /// Binds a key or an address to a person, pending until that person
    /// accepts (d7).
    Consent,
    /// An act against another admin person: at once, contestable, and the
    /// target's admin keys suspended (d8).
    AgainstAdmin,
    /// At once with nothing to settle (d9).
    Neutral,
}

impl AdminActClass {
    /// The spelling of `admin.act.class` and of the column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            AdminActClass::Narrowing => "narrowing",
            AdminActClass::Erase => "erase",
            AdminActClass::Widening => "widening",
            AdminActClass::Consent => "consent",
            AdminActClass::AgainstAdmin => "against_admin",
            AdminActClass::Neutral => "neutral",
        }
    }

    /// The class a spelling names; `None` for any other string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "narrowing" => AdminActClass::Narrowing,
            "erase" => AdminActClass::Erase,
            "widening" => AdminActClass::Widening,
            "consent" => AdminActClass::Consent,
            "against_admin" => AdminActClass::AgainstAdmin,
            "neutral" => AdminActClass::Neutral,
            _ => return None,
        })
    }
}

/// The states of ADR-0097 d2. `declined` and `expired` are terminal
/// (d23), and so is `settled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AdminActState {
    /// Took effect at once: narrowing, neutral, an act against an admin,
    /// and widening under `sole_admin` (marked).
    Effective,
    /// Widening that waits on a second admin person (d5, d23).
    PendingConfirmation,
    /// A second admin person confirmed it; it took effect then.
    Confirmed,
    /// Declined: a pending act never takes effect, and a `sole_admin` act
    /// is revoked (d10, d23).
    Declined,
    /// Unconfirmed past [`PENDING_LIFE_SECS`]: declined by expiry (d23).
    Expired,
    /// An act against an admin its target contested (d8).
    Contested,
    /// An act against an admin, settled by a third admin person or the
    /// operator (d8, d19).
    Settled,
}

impl AdminActState {
    /// The spelling of `admin.act.state` and of the column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            AdminActState::Effective => "effective",
            AdminActState::PendingConfirmation => "pending_confirmation",
            AdminActState::Confirmed => "confirmed",
            AdminActState::Declined => "declined",
            AdminActState::Expired => "expired",
            AdminActState::Contested => "contested",
            AdminActState::Settled => "settled",
        }
    }

    /// The state a spelling names; `None` for any other string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "effective" => AdminActState::Effective,
            "pending_confirmation" => AdminActState::PendingConfirmation,
            "confirmed" => AdminActState::Confirmed,
            "declined" => AdminActState::Declined,
            "expired" => AdminActState::Expired,
            "contested" => AdminActState::Contested,
            "settled" => AdminActState::Settled,
            _ => return None,
        })
    }

    /// Whether nothing moves the act on: declined, expired, settled.
    #[must_use]
    pub fn terminal(self) -> bool {
        matches!(
            self,
            AdminActState::Declined | AdminActState::Expired | AdminActState::Settled
        )
    }
}

/// A non-interactive origin of an act (ADR-0097 d2); `None` on the record
/// for an act an admin took.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AdminActCause {
    Upgrade,
    Deactivation,
    Settlement,
    Recovery,
}

impl AdminActCause {
    /// The spelling of `admin.act.cause` and of the column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            AdminActCause::Upgrade => "upgrade",
            AdminActCause::Deactivation => "deactivation",
            AdminActCause::Settlement => "settlement",
            AdminActCause::Recovery => "recovery",
        }
    }

    /// The cause a spelling names; `None` for any other string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "upgrade" => AdminActCause::Upgrade,
            "deactivation" => AdminActCause::Deactivation,
            "settlement" => AdminActCause::Settlement,
            "recovery" => AdminActCause::Recovery,
            _ => return None,
        })
    }
}

/// Every admin act the server journals, each with its one class
/// (ADR-0097 d3, d4, d5, d7, d8, d9, d14, d15). The spelling is
/// `admin.act.act`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AdminActKind {
    // Narrowing (d3).
    /// A per-key denial on any workspace (ADR-0096 d4).
    DenyKey,
    /// Revoking a key of no admin person (d14).
    RevokeKey,
    /// Revoking a connection.
    RevokeConnection,
    /// Disabling a person who holds no admin flag.
    DisableUser,
    /// Removing an entry that holds no admin flag.
    RemovePerson,
    /// Closing a deletion request; the person's request is the consent.
    CloseDeletion,
    /// A lower rate, an earlier expiry or a tighter quota (d15).
    LowerLimits,
    /// Declining a pending or `sole_admin` act (d3, d10).
    DeclineAct,
    // Erase (d4).
    /// Erasing a claim: the demotion at once, the shred after a second
    /// admin person.
    Erase,
    // Widening (d5).
    /// Issuing a key that can read, write, approve or ratify, admin and
    /// `maintenance` keys included.
    IssueKey,
    /// Adding a grant or a capacity, a person's root included (d6).
    AddGrant,
    /// A new admin person, or an admin flag on an existing person.
    AllowAdmin,
    /// Minting a connect code for any connector key (d6).
    MintConnectCode,
    /// A raise, a cleared limit, a later expiry or a looser quota (d15).
    RaiseLimits,
    // Consent (d7).
    /// Linking a key to a person, pending until that person accepts.
    LinkKey,
    // Against an admin (d8).
    /// Revoking a key of another admin person (d14).
    RevokeAdminKey,
    /// Un-flagging another admin person.
    UnflagAdmin,
    /// Removing another admin person's entry.
    RemoveAdmin,
    /// Disabling another admin person.
    DisableAdmin,
    // Neutral (d9).
    /// Creating a shared workspace (d9, d29).
    CreateWorkspace,
    /// Creating a crew.
    CreateCrew,
    /// Changing a crew's metadata.
    UpdateCrew,
    /// Setting a workspace's crew.
    SetWorkspaceCrew,
    /// An allowlist entry for a new person with no admin flag.
    AllowPerson,
    /// Changing the user of an entry that holds no admin flag.
    ChangePerson,
    /// A rescan of the tenant's held writes.
    Rescan,
    /// A key's new value, keeping exactly its access (d14's note).
    RotateKey,
}

/// Every kind, for the round-trip and coverage checks.
pub const ADMIN_ACT_KINDS: &[AdminActKind] = &[
    AdminActKind::DenyKey,
    AdminActKind::RevokeKey,
    AdminActKind::RevokeConnection,
    AdminActKind::DisableUser,
    AdminActKind::RemovePerson,
    AdminActKind::CloseDeletion,
    AdminActKind::LowerLimits,
    AdminActKind::DeclineAct,
    AdminActKind::Erase,
    AdminActKind::IssueKey,
    AdminActKind::AddGrant,
    AdminActKind::AllowAdmin,
    AdminActKind::MintConnectCode,
    AdminActKind::RaiseLimits,
    AdminActKind::LinkKey,
    AdminActKind::RevokeAdminKey,
    AdminActKind::UnflagAdmin,
    AdminActKind::RemoveAdmin,
    AdminActKind::DisableAdmin,
    AdminActKind::CreateWorkspace,
    AdminActKind::CreateCrew,
    AdminActKind::UpdateCrew,
    AdminActKind::SetWorkspaceCrew,
    AdminActKind::AllowPerson,
    AdminActKind::ChangePerson,
    AdminActKind::Rescan,
    AdminActKind::RotateKey,
];

impl AdminActKind {
    /// The act's one class, fixed here (ADR-0097 d2).
    #[must_use]
    pub fn class(self) -> AdminActClass {
        use AdminActKind as K;
        match self {
            K::DenyKey
            | K::RevokeKey
            | K::RevokeConnection
            | K::DisableUser
            | K::RemovePerson
            | K::CloseDeletion
            | K::LowerLimits
            | K::DeclineAct => AdminActClass::Narrowing,
            K::Erase => AdminActClass::Erase,
            K::IssueKey | K::AddGrant | K::AllowAdmin | K::MintConnectCode | K::RaiseLimits => {
                AdminActClass::Widening
            }
            K::LinkKey => AdminActClass::Consent,
            K::RevokeAdminKey | K::UnflagAdmin | K::RemoveAdmin | K::DisableAdmin => {
                AdminActClass::AgainstAdmin
            }
            K::CreateWorkspace
            | K::CreateCrew
            | K::UpdateCrew
            | K::SetWorkspaceCrew
            | K::AllowPerson
            | K::ChangePerson
            | K::Rescan
            | K::RotateKey => AdminActClass::Neutral,
        }
    }

    /// The spelling of `admin.act.act` and of the column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        use AdminActKind as K;
        match self {
            K::DenyKey => "deny_key",
            K::RevokeKey => "revoke_key",
            K::RevokeConnection => "revoke_connection",
            K::DisableUser => "disable_user",
            K::RemovePerson => "remove_person",
            K::CloseDeletion => "close_deletion",
            K::LowerLimits => "lower_limits",
            K::DeclineAct => "decline_act",
            K::Erase => "erase",
            K::IssueKey => "issue_key",
            K::AddGrant => "add_grant",
            K::AllowAdmin => "allow_admin",
            K::MintConnectCode => "mint_connect_code",
            K::RaiseLimits => "raise_limits",
            K::LinkKey => "link_key",
            K::RevokeAdminKey => "revoke_admin_key",
            K::UnflagAdmin => "unflag_admin",
            K::RemoveAdmin => "remove_admin",
            K::DisableAdmin => "disable_admin",
            K::CreateWorkspace => "create_workspace",
            K::CreateCrew => "create_crew",
            K::UpdateCrew => "update_crew",
            K::SetWorkspaceCrew => "set_workspace_crew",
            K::AllowPerson => "allow_person",
            K::ChangePerson => "change_person",
            K::Rescan => "rescan",
            K::RotateKey => "rotate_key",
        }
    }

    /// The act a spelling names; `None` for any other string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        ADMIN_ACT_KINDS.iter().copied().find(|k| k.as_str() == s)
    }

    /// Whether the act requires a reason on the wire (ADR-0097 d26):
    /// widening, consent and against-an-admin acts do; narrowing, erase and
    /// neutral acts take a missing one as `null`.
    #[must_use]
    pub fn requires_reason(self) -> bool {
        matches!(
            self.class(),
            AdminActClass::Widening | AdminActClass::Consent | AdminActClass::AgainstAdmin
        )
    }
}

/// The age an admin person's entry must reach, from the acceptance of its
/// admin flag, before that person counts as a second admin person: seven
/// days, a placeholder, chosen, not measured (ADR-0097 d11).
pub const SECOND_ADMIN_AGE_SECS: i64 = 7 * 86_400;

/// How long a pending widening act waits before it expires as declined:
/// seven days, a placeholder that must exceed the notice escalation hours
/// (ADR-0097 d23).
pub const PENDING_LIFE_SECS: i64 = 7 * 86_400;

/// Whether an admin person whose admin flag was accepted at
/// `admin_since_micros` counts as a second admin person for an act taken at
/// `act_at_micros` (ADR-0097 d11): accepted at least
/// [`SECOND_ADMIN_AGE_SECS`] before the act.
#[must_use]
pub fn counts_for_act(admin_since_micros: i64, act_at_micros: i64) -> bool {
    act_at_micros - admin_since_micros >= SECOND_ADMIN_AGE_SECS * 1_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_its_spelling_and_sits_in_one_class() {
        for kind in ADMIN_ACT_KINDS {
            assert_eq!(AdminActKind::parse(kind.as_str()), Some(*kind));
            let class = kind.class();
            assert_eq!(AdminActClass::parse(class.as_str()), Some(class));
        }
        let mut spellings: Vec<&str> = ADMIN_ACT_KINDS.iter().map(|k| k.as_str()).collect();
        spellings.sort_unstable();
        spellings.dedup();
        assert_eq!(spellings.len(), ADMIN_ACT_KINDS.len());
    }

    #[test]
    fn every_state_round_trips_and_only_declined_expired_and_settled_are_terminal() {
        for state in [
            AdminActState::Effective,
            AdminActState::PendingConfirmation,
            AdminActState::Confirmed,
            AdminActState::Declined,
            AdminActState::Expired,
            AdminActState::Contested,
            AdminActState::Settled,
        ] {
            assert_eq!(AdminActState::parse(state.as_str()), Some(state));
            assert_eq!(
                state.terminal(),
                matches!(
                    state,
                    AdminActState::Declined | AdminActState::Expired | AdminActState::Settled
                )
            );
        }
    }

    #[test]
    fn an_admin_person_accepted_less_than_seven_days_before_the_act_does_not_count() {
        let day = 86_400 * 1_000_000_i64;
        assert!(!counts_for_act(0, 7 * day - 1));
        assert!(counts_for_act(0, 7 * day));
    }

    #[test]
    fn a_revocation_of_an_admin_persons_key_is_against_an_admin_and_any_other_narrowing() {
        assert_eq!(AdminActKind::RevokeKey.class(), AdminActClass::Narrowing);
        assert_eq!(
            AdminActKind::RevokeAdminKey.class(),
            AdminActClass::AgainstAdmin
        );
        assert_eq!(AdminActKind::RaiseLimits.class(), AdminActClass::Widening);
        assert_eq!(AdminActKind::LowerLimits.class(), AdminActClass::Narrowing);
    }
}
