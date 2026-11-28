from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.rescan_report_by_rule_item import RescanReportByRuleItem
    from ..models.rescan_report_findings_item import RescanReportFindingsItem
    from ..models.rescan_report_flagged_creates_item import (
        RescanReportFlaggedCreatesItem,
    )
    from ..models.rescan_report_public_workspaces_item import (
        RescanReportPublicWorkspacesItem,
    )
    from ..models.rescan_report_scanned_by_origin import RescanReportScannedByOrigin


T = TypeVar("T", bound="RescanReport")


@_attrs_define
class RescanReport:
    """One rescan window (ADR-0065 d13). No field carries a version's text, claim key or anchor value.

    Attributes:
        scanner_version (None | str):
        scanned (int): Versions scanned in this window: every stored state but erased.
        findings (list[RescanReportFindingsItem]):
        by_rule (list[RescanReportByRuleItem]):
        next_after_seq (int | None): The after_seq of the next window, or null at the chain head.
        recorded (int): Held versions whose findings this call recorded under the server's rule set (ADR-0065 d15;
            ADR-0074 d28); 0 for the read.
        held_unrescanned (int): After this call, the tenant's held versions last scanned under another rule set than the
            server's, which stay their authors' alone in in-progress workspaces: doctor's scan_held_unrescanned (ADR-0074
            d28).
        rescan_due (int): After this call, every stored version, erased ones aside, last scanned under another rule set:
            doctor's scan_rescan_due (ADR-0074 d28).
        public_workspaces (list[RescanReportPublicWorkspacesItem] | Unset): The tenant's public workspaces, for doctor's
            workspace.public: each one's flag and served rows, the rows every default recall of the tenant also searches
            (ADR-0098 d9). Since v0.8.6.
        public_ineligible (list[UUID] | Unset): Public workspaces that are development or inside a user root, which only
            a direct store edit produces: doctor's workspace.public_ineligible, critical (ADR-0098 d9). Since v0.8.6.
        flagged_creates (list[RescanReportFlaggedCreatesItem] | Unset): Per creating key, the workspaces it created in
            the shared tree under a grant carrying create, which no account's workspaces meter counts: doctor's
            workspace.flagged_creates, info (ADR-0097 d31, the operator's decision of 2026-11-27). Since v0.8.6.
        scanned_by_origin (RescanReportScannedByOrigin | Unset): The versions scanned, counted by origin kind, none for
            a version stored without one: the denominators of the dry run's rates (ADR-0074 d18).
        findings_unlisted (int | Unset): After this call, held versions whose stored scan finding has no row in the
            findings list, written by a server older than migration 0043: doctor's scan_findings_unlisted (ADR-0074 d23).
        promoted_unapproved (int | Unset): After this call, served versions (promoted tips) with no approval: their
            approved_at is null because migration 0044's fill could not match the verdict their promotion names. A version a
            ratified successor replaced is not counted. Doctor's approval_unmatched (ADR-0077 d6). A count, never a list.
        instructions_without_person_approval (int | Unset): After this call, served instruction-class versions (promoted
            steering, guardrail and skill tips, retirements aside) whose approving key was not a person's reviewer key, or
            that carry no approval: approved before an instruction became a person's to approve. A person reviews one again
            by superseding it; the replaced version is then no tip and not counted. Read off the row's approver_used_by, so
            a person's key that had a crew is not counted. Doctor's instruction_approvals (ADR-0080 d16, #436). Since
            v0.8.3.
        withdrawn_in_queue (int | Unset): After this call, withdrawn versions still in a review queue, which a
            withdrawal's own transaction rules out: doctor's memory.withdrawn_in_queue (ADR-0083 d13).
        held_successors_stranded (int | Unset): Held successors of a version that is no longer its claim's promoted tip,
            so no verdict can approve them: doctor's memory.held_successor_stranded (ADR-0089 d16).
        erased_keys_live (int | Unset): Erased versions whose data key is still live, which the erase's own transaction
            and the lake writer's fold rule out: doctor's lake.erased_key_live (ADR-0005 d2).
        investigations_open_long (int | Unset): Admin investigations open for more than seven days: doctor's
            admin.investigation_open_long (ADR-0089 d10).
        investigations_past_pages (int | Unset): Open admin investigations past 50 pages, a placeholder: doctor's
            admin.investigation_pages (ADR-0097 d16).
        fixes_unconfirmed (int | Unset): Admin fixes no person has confirmed or reverted (ADR-0089 d15).
        fix_oldest_unconfirmed_seconds (int | Unset): The oldest unconfirmed fix's age in seconds, 0 with none.
        fixes_loosening_guardrail (int | Unset): Unconfirmed fixes that loosen a guardrail (ADR-0089 d15).
        fixes_without_person_reviewer (int | Unset): Unconfirmed fixes on a workspace where no live person's reviewer
            key holds reviewer.
        fixes_admin_person_only (int | Unset): Unconfirmed fixes only the fixing admin's own person could settle.
        fixes_reverted (int | Unset): Fixes a person reverted.
        key_links_pending (int | Unset): Live keys whose link to a person waits on that person's acceptance (ADR-0085
            d16).
        web_emails_pending (int | Unset): Allowlist entries that wait on their person's acceptance.
        admin_acts_pending (int | Unset): Admin acts that wait on a second admin person: doctor's
            admin.act_pending_confirmation (ADR-0097 d5).
        admin_act_oldest_pending_seconds (int | Unset): The oldest pending admin act's age in seconds, 0 with none.
        admin_erase_shreds_pending (int | Unset): Erases whose shred waits on a second admin person, their bytes kept:
            doctor's admin.erase_shred_pending (ADR-0097 d4). Not counted in admin_acts_pending, since a pending shred never
            expires. Since v0.8.6.
        admin_erase_shred_oldest_seconds (int | Unset): The oldest pending shred's age in seconds, 0 with none (ADR-0097
            d4). Since v0.8.6.
        admin_acts_sole_admin (int | Unset): Widening acts taken at once under sole_admin: doctor's admin.act_sole_admin
            (ADR-0097 d10).
        sole_admin (bool | Unset): Whether the tenant has never had two live admin persons (ADR-0097 d10).
        admin_acts_without_reason (int | Unset): Admin acts journalled with no reason: doctor's admin.act_without_reason
            (ADR-0097 d26).
        admin_acts_expired (int | Unset): Pending admin acts that expired as declined in the last seven days: doctor's
            admin.act_expired (ADR-0097 d23).
        admin_acts_offensive (int | Unset): Offensive narrowing in the last 24 hours: doctor's admin.offensive_narrowing
            (ADR-0097 d24).
        admin_timeboxes_unsettled (int | Unset): Time boxes no second admin person has settled, open or ended: doctor's
            admin.timebox_open (ADR-0097 d13).
        admin_narrowing_burst (int | Unset): The most narrowing acts one admin person took in the last 24 hours:
            doctor's admin.narrowing_burst (ADR-0097 d24).
        admin_keys_suspended (int | Unset): Admin keys suspended while an act against their person is open: doctor's
            admin.key_suspended (ADR-0097 d8, d21). Since v0.8.6.
        admin_keys_unlinked_suspended (int | Unset): Admin keys suspended as unlinked at the upgrade, which reach only
            /v2/whoami and POST /v2/keys/self/link: doctor's admin.key_unlinked_suspended (ADR-0097 d21). Since v0.8.6.
        admin_unlinked_days_left (int | Unset): The fewest days an unlinked admin key has left before it is revoked, 0
            with none. Since v0.8.6.
        admin_disputes_open (int | Unset): Open acts against an admin below three admin persons, a freeze the operator
            settles: doctor's admin.dispute_open, critical (ADR-0097 d8). Since v0.8.6.
        operator_settlements_due (int | Unset): Freezes open at least 24 hours, a placeholder, which the operator may
            settle from the host with oml operator settle: doctor's operator.settlement_due (ADR-0097 d19). Since v0.8.6.
        key_issuer_unexpected_shape (int | Unset): Keys the tenant's server issuer issued that are not one of the four
            session-key shapes (<user>-web, <user>-review, <user>-curate, <user>-admin): doctor's
            key.issuer_unexpected_shape, critical (ADR-0097 d18). Since v0.8.6.
        mail_unconfigured (int | Unset): 1 when no mail relay is configured (mail.host and mail.from unset), so recovery
            and settlement notices are journalled and shown in-product only: doctor's notice.mail_unconfigured (ADR-0097
            d20). Since v0.8.6.
        mail_delivery_failures (int | Unset): Notice emails the relay refused or did not take since the lead started;
            the acts they told about stand: doctor's notice.mail_delivery_failed (ADR-0097 d20). Since v0.8.6.
        mail_last_failure (None | str | Unset): The last failed delivery's reason, naming the SMTP stage and code and
            never an address. Since v0.8.6.
    """

    scanner_version: None | str
    scanned: int
    findings: list[RescanReportFindingsItem]
    by_rule: list[RescanReportByRuleItem]
    next_after_seq: int | None
    recorded: int
    held_unrescanned: int
    rescan_due: int
    public_workspaces: list[RescanReportPublicWorkspacesItem] | Unset = UNSET
    public_ineligible: list[UUID] | Unset = UNSET
    flagged_creates: list[RescanReportFlaggedCreatesItem] | Unset = UNSET
    scanned_by_origin: RescanReportScannedByOrigin | Unset = UNSET
    findings_unlisted: int | Unset = UNSET
    promoted_unapproved: int | Unset = UNSET
    instructions_without_person_approval: int | Unset = UNSET
    withdrawn_in_queue: int | Unset = UNSET
    held_successors_stranded: int | Unset = UNSET
    erased_keys_live: int | Unset = UNSET
    investigations_open_long: int | Unset = UNSET
    investigations_past_pages: int | Unset = UNSET
    fixes_unconfirmed: int | Unset = UNSET
    fix_oldest_unconfirmed_seconds: int | Unset = UNSET
    fixes_loosening_guardrail: int | Unset = UNSET
    fixes_without_person_reviewer: int | Unset = UNSET
    fixes_admin_person_only: int | Unset = UNSET
    fixes_reverted: int | Unset = UNSET
    key_links_pending: int | Unset = UNSET
    web_emails_pending: int | Unset = UNSET
    admin_acts_pending: int | Unset = UNSET
    admin_act_oldest_pending_seconds: int | Unset = UNSET
    admin_erase_shreds_pending: int | Unset = UNSET
    admin_erase_shred_oldest_seconds: int | Unset = UNSET
    admin_acts_sole_admin: int | Unset = UNSET
    sole_admin: bool | Unset = UNSET
    admin_acts_without_reason: int | Unset = UNSET
    admin_acts_expired: int | Unset = UNSET
    admin_acts_offensive: int | Unset = UNSET
    admin_timeboxes_unsettled: int | Unset = UNSET
    admin_narrowing_burst: int | Unset = UNSET
    admin_keys_suspended: int | Unset = UNSET
    admin_keys_unlinked_suspended: int | Unset = UNSET
    admin_unlinked_days_left: int | Unset = UNSET
    admin_disputes_open: int | Unset = UNSET
    operator_settlements_due: int | Unset = UNSET
    key_issuer_unexpected_shape: int | Unset = UNSET
    mail_unconfigured: int | Unset = UNSET
    mail_delivery_failures: int | Unset = UNSET
    mail_last_failure: None | str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        scanner_version: None | str
        scanner_version = self.scanner_version

        scanned = self.scanned

        findings = []
        for findings_item_data in self.findings:
            findings_item = findings_item_data.to_dict()
            findings.append(findings_item)

        by_rule = []
        for by_rule_item_data in self.by_rule:
            by_rule_item = by_rule_item_data.to_dict()
            by_rule.append(by_rule_item)

        next_after_seq: int | None
        next_after_seq = self.next_after_seq

        recorded = self.recorded

        held_unrescanned = self.held_unrescanned

        rescan_due = self.rescan_due

        public_workspaces: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.public_workspaces, Unset):
            public_workspaces = []
            for public_workspaces_item_data in self.public_workspaces:
                public_workspaces_item = public_workspaces_item_data.to_dict()
                public_workspaces.append(public_workspaces_item)

        public_ineligible: list[str] | Unset = UNSET
        if not isinstance(self.public_ineligible, Unset):
            public_ineligible = []
            for public_ineligible_item_data in self.public_ineligible:
                public_ineligible_item = str(public_ineligible_item_data)
                public_ineligible.append(public_ineligible_item)

        flagged_creates: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.flagged_creates, Unset):
            flagged_creates = []
            for flagged_creates_item_data in self.flagged_creates:
                flagged_creates_item = flagged_creates_item_data.to_dict()
                flagged_creates.append(flagged_creates_item)

        scanned_by_origin: dict[str, Any] | Unset = UNSET
        if not isinstance(self.scanned_by_origin, Unset):
            scanned_by_origin = self.scanned_by_origin.to_dict()

        findings_unlisted = self.findings_unlisted

        promoted_unapproved = self.promoted_unapproved

        instructions_without_person_approval = self.instructions_without_person_approval

        withdrawn_in_queue = self.withdrawn_in_queue

        held_successors_stranded = self.held_successors_stranded

        erased_keys_live = self.erased_keys_live

        investigations_open_long = self.investigations_open_long

        investigations_past_pages = self.investigations_past_pages

        fixes_unconfirmed = self.fixes_unconfirmed

        fix_oldest_unconfirmed_seconds = self.fix_oldest_unconfirmed_seconds

        fixes_loosening_guardrail = self.fixes_loosening_guardrail

        fixes_without_person_reviewer = self.fixes_without_person_reviewer

        fixes_admin_person_only = self.fixes_admin_person_only

        fixes_reverted = self.fixes_reverted

        key_links_pending = self.key_links_pending

        web_emails_pending = self.web_emails_pending

        admin_acts_pending = self.admin_acts_pending

        admin_act_oldest_pending_seconds = self.admin_act_oldest_pending_seconds

        admin_erase_shreds_pending = self.admin_erase_shreds_pending

        admin_erase_shred_oldest_seconds = self.admin_erase_shred_oldest_seconds

        admin_acts_sole_admin = self.admin_acts_sole_admin

        sole_admin = self.sole_admin

        admin_acts_without_reason = self.admin_acts_without_reason

        admin_acts_expired = self.admin_acts_expired

        admin_acts_offensive = self.admin_acts_offensive

        admin_timeboxes_unsettled = self.admin_timeboxes_unsettled

        admin_narrowing_burst = self.admin_narrowing_burst

        admin_keys_suspended = self.admin_keys_suspended

        admin_keys_unlinked_suspended = self.admin_keys_unlinked_suspended

        admin_unlinked_days_left = self.admin_unlinked_days_left

        admin_disputes_open = self.admin_disputes_open

        operator_settlements_due = self.operator_settlements_due

        key_issuer_unexpected_shape = self.key_issuer_unexpected_shape

        mail_unconfigured = self.mail_unconfigured

        mail_delivery_failures = self.mail_delivery_failures

        mail_last_failure: None | str | Unset
        if isinstance(self.mail_last_failure, Unset):
            mail_last_failure = UNSET
        else:
            mail_last_failure = self.mail_last_failure

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "scanner_version": scanner_version,
                "scanned": scanned,
                "findings": findings,
                "by_rule": by_rule,
                "next_after_seq": next_after_seq,
                "recorded": recorded,
                "held_unrescanned": held_unrescanned,
                "rescan_due": rescan_due,
            }
        )
        if public_workspaces is not UNSET:
            field_dict["public_workspaces"] = public_workspaces
        if public_ineligible is not UNSET:
            field_dict["public_ineligible"] = public_ineligible
        if flagged_creates is not UNSET:
            field_dict["flagged_creates"] = flagged_creates
        if scanned_by_origin is not UNSET:
            field_dict["scanned_by_origin"] = scanned_by_origin
        if findings_unlisted is not UNSET:
            field_dict["findings_unlisted"] = findings_unlisted
        if promoted_unapproved is not UNSET:
            field_dict["promoted_unapproved"] = promoted_unapproved
        if instructions_without_person_approval is not UNSET:
            field_dict["instructions_without_person_approval"] = (
                instructions_without_person_approval
            )
        if withdrawn_in_queue is not UNSET:
            field_dict["withdrawn_in_queue"] = withdrawn_in_queue
        if held_successors_stranded is not UNSET:
            field_dict["held_successors_stranded"] = held_successors_stranded
        if erased_keys_live is not UNSET:
            field_dict["erased_keys_live"] = erased_keys_live
        if investigations_open_long is not UNSET:
            field_dict["investigations_open_long"] = investigations_open_long
        if investigations_past_pages is not UNSET:
            field_dict["investigations_past_pages"] = investigations_past_pages
        if fixes_unconfirmed is not UNSET:
            field_dict["fixes_unconfirmed"] = fixes_unconfirmed
        if fix_oldest_unconfirmed_seconds is not UNSET:
            field_dict["fix_oldest_unconfirmed_seconds"] = (
                fix_oldest_unconfirmed_seconds
            )
        if fixes_loosening_guardrail is not UNSET:
            field_dict["fixes_loosening_guardrail"] = fixes_loosening_guardrail
        if fixes_without_person_reviewer is not UNSET:
            field_dict["fixes_without_person_reviewer"] = fixes_without_person_reviewer
        if fixes_admin_person_only is not UNSET:
            field_dict["fixes_admin_person_only"] = fixes_admin_person_only
        if fixes_reverted is not UNSET:
            field_dict["fixes_reverted"] = fixes_reverted
        if key_links_pending is not UNSET:
            field_dict["key_links_pending"] = key_links_pending
        if web_emails_pending is not UNSET:
            field_dict["web_emails_pending"] = web_emails_pending
        if admin_acts_pending is not UNSET:
            field_dict["admin_acts_pending"] = admin_acts_pending
        if admin_act_oldest_pending_seconds is not UNSET:
            field_dict["admin_act_oldest_pending_seconds"] = (
                admin_act_oldest_pending_seconds
            )
        if admin_erase_shreds_pending is not UNSET:
            field_dict["admin_erase_shreds_pending"] = admin_erase_shreds_pending
        if admin_erase_shred_oldest_seconds is not UNSET:
            field_dict["admin_erase_shred_oldest_seconds"] = (
                admin_erase_shred_oldest_seconds
            )
        if admin_acts_sole_admin is not UNSET:
            field_dict["admin_acts_sole_admin"] = admin_acts_sole_admin
        if sole_admin is not UNSET:
            field_dict["sole_admin"] = sole_admin
        if admin_acts_without_reason is not UNSET:
            field_dict["admin_acts_without_reason"] = admin_acts_without_reason
        if admin_acts_expired is not UNSET:
            field_dict["admin_acts_expired"] = admin_acts_expired
        if admin_acts_offensive is not UNSET:
            field_dict["admin_acts_offensive"] = admin_acts_offensive
        if admin_timeboxes_unsettled is not UNSET:
            field_dict["admin_timeboxes_unsettled"] = admin_timeboxes_unsettled
        if admin_narrowing_burst is not UNSET:
            field_dict["admin_narrowing_burst"] = admin_narrowing_burst
        if admin_keys_suspended is not UNSET:
            field_dict["admin_keys_suspended"] = admin_keys_suspended
        if admin_keys_unlinked_suspended is not UNSET:
            field_dict["admin_keys_unlinked_suspended"] = admin_keys_unlinked_suspended
        if admin_unlinked_days_left is not UNSET:
            field_dict["admin_unlinked_days_left"] = admin_unlinked_days_left
        if admin_disputes_open is not UNSET:
            field_dict["admin_disputes_open"] = admin_disputes_open
        if operator_settlements_due is not UNSET:
            field_dict["operator_settlements_due"] = operator_settlements_due
        if key_issuer_unexpected_shape is not UNSET:
            field_dict["key_issuer_unexpected_shape"] = key_issuer_unexpected_shape
        if mail_unconfigured is not UNSET:
            field_dict["mail_unconfigured"] = mail_unconfigured
        if mail_delivery_failures is not UNSET:
            field_dict["mail_delivery_failures"] = mail_delivery_failures
        if mail_last_failure is not UNSET:
            field_dict["mail_last_failure"] = mail_last_failure

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.rescan_report_by_rule_item import RescanReportByRuleItem
        from ..models.rescan_report_findings_item import RescanReportFindingsItem
        from ..models.rescan_report_flagged_creates_item import (
            RescanReportFlaggedCreatesItem,
        )
        from ..models.rescan_report_public_workspaces_item import (
            RescanReportPublicWorkspacesItem,
        )
        from ..models.rescan_report_scanned_by_origin import RescanReportScannedByOrigin

        d = dict(src_dict)

        def _parse_scanner_version(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        scanner_version = _parse_scanner_version(d.pop("scanner_version"))

        scanned = d.pop("scanned")

        findings = []
        _findings = d.pop("findings")
        for findings_item_data in _findings:
            findings_item = RescanReportFindingsItem.from_dict(findings_item_data)

            findings.append(findings_item)

        by_rule = []
        _by_rule = d.pop("by_rule")
        for by_rule_item_data in _by_rule:
            by_rule_item = RescanReportByRuleItem.from_dict(by_rule_item_data)

            by_rule.append(by_rule_item)

        def _parse_next_after_seq(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        next_after_seq = _parse_next_after_seq(d.pop("next_after_seq"))

        recorded = d.pop("recorded")

        held_unrescanned = d.pop("held_unrescanned")

        rescan_due = d.pop("rescan_due")

        _public_workspaces = d.pop("public_workspaces", UNSET)
        public_workspaces: list[RescanReportPublicWorkspacesItem] | Unset = UNSET
        if _public_workspaces is not UNSET:
            public_workspaces = []
            for public_workspaces_item_data in _public_workspaces:
                public_workspaces_item = RescanReportPublicWorkspacesItem.from_dict(
                    public_workspaces_item_data
                )

                public_workspaces.append(public_workspaces_item)

        _public_ineligible = d.pop("public_ineligible", UNSET)
        public_ineligible: list[UUID] | Unset = UNSET
        if _public_ineligible is not UNSET:
            public_ineligible = []
            for public_ineligible_item_data in _public_ineligible:
                public_ineligible_item = UUID(public_ineligible_item_data)

                public_ineligible.append(public_ineligible_item)

        _flagged_creates = d.pop("flagged_creates", UNSET)
        flagged_creates: list[RescanReportFlaggedCreatesItem] | Unset = UNSET
        if _flagged_creates is not UNSET:
            flagged_creates = []
            for flagged_creates_item_data in _flagged_creates:
                flagged_creates_item = RescanReportFlaggedCreatesItem.from_dict(
                    flagged_creates_item_data
                )

                flagged_creates.append(flagged_creates_item)

        _scanned_by_origin = d.pop("scanned_by_origin", UNSET)
        scanned_by_origin: RescanReportScannedByOrigin | Unset
        if isinstance(_scanned_by_origin, Unset):
            scanned_by_origin = UNSET
        else:
            scanned_by_origin = RescanReportScannedByOrigin.from_dict(
                _scanned_by_origin
            )

        findings_unlisted = d.pop("findings_unlisted", UNSET)

        promoted_unapproved = d.pop("promoted_unapproved", UNSET)

        instructions_without_person_approval = d.pop(
            "instructions_without_person_approval", UNSET
        )

        withdrawn_in_queue = d.pop("withdrawn_in_queue", UNSET)

        held_successors_stranded = d.pop("held_successors_stranded", UNSET)

        erased_keys_live = d.pop("erased_keys_live", UNSET)

        investigations_open_long = d.pop("investigations_open_long", UNSET)

        investigations_past_pages = d.pop("investigations_past_pages", UNSET)

        fixes_unconfirmed = d.pop("fixes_unconfirmed", UNSET)

        fix_oldest_unconfirmed_seconds = d.pop("fix_oldest_unconfirmed_seconds", UNSET)

        fixes_loosening_guardrail = d.pop("fixes_loosening_guardrail", UNSET)

        fixes_without_person_reviewer = d.pop("fixes_without_person_reviewer", UNSET)

        fixes_admin_person_only = d.pop("fixes_admin_person_only", UNSET)

        fixes_reverted = d.pop("fixes_reverted", UNSET)

        key_links_pending = d.pop("key_links_pending", UNSET)

        web_emails_pending = d.pop("web_emails_pending", UNSET)

        admin_acts_pending = d.pop("admin_acts_pending", UNSET)

        admin_act_oldest_pending_seconds = d.pop(
            "admin_act_oldest_pending_seconds", UNSET
        )

        admin_erase_shreds_pending = d.pop("admin_erase_shreds_pending", UNSET)

        admin_erase_shred_oldest_seconds = d.pop(
            "admin_erase_shred_oldest_seconds", UNSET
        )

        admin_acts_sole_admin = d.pop("admin_acts_sole_admin", UNSET)

        sole_admin = d.pop("sole_admin", UNSET)

        admin_acts_without_reason = d.pop("admin_acts_without_reason", UNSET)

        admin_acts_expired = d.pop("admin_acts_expired", UNSET)

        admin_acts_offensive = d.pop("admin_acts_offensive", UNSET)

        admin_timeboxes_unsettled = d.pop("admin_timeboxes_unsettled", UNSET)

        admin_narrowing_burst = d.pop("admin_narrowing_burst", UNSET)

        admin_keys_suspended = d.pop("admin_keys_suspended", UNSET)

        admin_keys_unlinked_suspended = d.pop("admin_keys_unlinked_suspended", UNSET)

        admin_unlinked_days_left = d.pop("admin_unlinked_days_left", UNSET)

        admin_disputes_open = d.pop("admin_disputes_open", UNSET)

        operator_settlements_due = d.pop("operator_settlements_due", UNSET)

        key_issuer_unexpected_shape = d.pop("key_issuer_unexpected_shape", UNSET)

        mail_unconfigured = d.pop("mail_unconfigured", UNSET)

        mail_delivery_failures = d.pop("mail_delivery_failures", UNSET)

        def _parse_mail_last_failure(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mail_last_failure = _parse_mail_last_failure(d.pop("mail_last_failure", UNSET))

        rescan_report = cls(
            scanner_version=scanner_version,
            scanned=scanned,
            findings=findings,
            by_rule=by_rule,
            next_after_seq=next_after_seq,
            recorded=recorded,
            held_unrescanned=held_unrescanned,
            rescan_due=rescan_due,
            public_workspaces=public_workspaces,
            public_ineligible=public_ineligible,
            flagged_creates=flagged_creates,
            scanned_by_origin=scanned_by_origin,
            findings_unlisted=findings_unlisted,
            promoted_unapproved=promoted_unapproved,
            instructions_without_person_approval=instructions_without_person_approval,
            withdrawn_in_queue=withdrawn_in_queue,
            held_successors_stranded=held_successors_stranded,
            erased_keys_live=erased_keys_live,
            investigations_open_long=investigations_open_long,
            investigations_past_pages=investigations_past_pages,
            fixes_unconfirmed=fixes_unconfirmed,
            fix_oldest_unconfirmed_seconds=fix_oldest_unconfirmed_seconds,
            fixes_loosening_guardrail=fixes_loosening_guardrail,
            fixes_without_person_reviewer=fixes_without_person_reviewer,
            fixes_admin_person_only=fixes_admin_person_only,
            fixes_reverted=fixes_reverted,
            key_links_pending=key_links_pending,
            web_emails_pending=web_emails_pending,
            admin_acts_pending=admin_acts_pending,
            admin_act_oldest_pending_seconds=admin_act_oldest_pending_seconds,
            admin_erase_shreds_pending=admin_erase_shreds_pending,
            admin_erase_shred_oldest_seconds=admin_erase_shred_oldest_seconds,
            admin_acts_sole_admin=admin_acts_sole_admin,
            sole_admin=sole_admin,
            admin_acts_without_reason=admin_acts_without_reason,
            admin_acts_expired=admin_acts_expired,
            admin_acts_offensive=admin_acts_offensive,
            admin_timeboxes_unsettled=admin_timeboxes_unsettled,
            admin_narrowing_burst=admin_narrowing_burst,
            admin_keys_suspended=admin_keys_suspended,
            admin_keys_unlinked_suspended=admin_keys_unlinked_suspended,
            admin_unlinked_days_left=admin_unlinked_days_left,
            admin_disputes_open=admin_disputes_open,
            operator_settlements_due=operator_settlements_due,
            key_issuer_unexpected_shape=key_issuer_unexpected_shape,
            mail_unconfigured=mail_unconfigured,
            mail_delivery_failures=mail_delivery_failures,
            mail_last_failure=mail_last_failure,
        )

        return rescan_report
