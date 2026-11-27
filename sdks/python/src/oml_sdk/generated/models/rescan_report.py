from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.rescan_report_by_rule_item import RescanReportByRuleItem
    from ..models.rescan_report_findings_item import RescanReportFindingsItem
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
    scanned_by_origin: RescanReportScannedByOrigin | Unset = UNSET
    findings_unlisted: int | Unset = UNSET
    promoted_unapproved: int | Unset = UNSET
    instructions_without_person_approval: int | Unset = UNSET

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

        scanned_by_origin: dict[str, Any] | Unset = UNSET
        if not isinstance(self.scanned_by_origin, Unset):
            scanned_by_origin = self.scanned_by_origin.to_dict()

        findings_unlisted = self.findings_unlisted

        promoted_unapproved = self.promoted_unapproved

        instructions_without_person_approval = self.instructions_without_person_approval

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

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.rescan_report_by_rule_item import RescanReportByRuleItem
        from ..models.rescan_report_findings_item import RescanReportFindingsItem
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
            scanned_by_origin=scanned_by_origin,
            findings_unlisted=findings_unlisted,
            promoted_unapproved=promoted_unapproved,
            instructions_without_person_approval=instructions_without_person_approval,
        )

        return rescan_report
