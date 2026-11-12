from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.rescan_report_by_rule_item import RescanReportByRuleItem
    from ..models.rescan_report_findings_item import RescanReportFindingsItem
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
            server's, which stay their authors' alone in in-progress workareas: doctor's scan_held_unrescanned (ADR-0074
            d28).
        rescan_due (int): After this call, every stored version, erased ones aside, last scanned under another rule set:
            doctor's scan_rescan_due (ADR-0074 d28).
        scanned_by_origin (RescanReportScannedByOrigin | Unset): The versions scanned, counted by origin kind, none for
            a version stored without one: the denominators of the dry run's rates (ADR-0074 d18).
        findings_unlisted (int | Unset): After this call, held versions whose stored scan finding has no row in the
            findings list, written by a server older than migration 0043: doctor's scan_findings_unlisted (ADR-0074 d23).
        promoted_unapproved (int | Unset): After this call, served versions (promoted tips) with no approval: their
            approved_at is null because migration 0044's fill could not match the verdict their promotion names. A version a
            ratified successor replaced is not counted. Doctor's approval_unmatched (ADR-0077 d6). A count, never a list.
    """

    scanner_version: None | str
    scanned: int
    findings: list[RescanReportFindingsItem]
    by_rule: list[RescanReportByRuleItem]
    next_after_seq: int | None
    recorded: int
    held_unrescanned: int
    rescan_due: int
    scanned_by_origin: RescanReportScannedByOrigin | Unset = UNSET
    findings_unlisted: int | Unset = UNSET
    promoted_unapproved: int | Unset = UNSET

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

        scanned_by_origin: dict[str, Any] | Unset = UNSET
        if not isinstance(self.scanned_by_origin, Unset):
            scanned_by_origin = self.scanned_by_origin.to_dict()

        findings_unlisted = self.findings_unlisted

        promoted_unapproved = self.promoted_unapproved

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
        if scanned_by_origin is not UNSET:
            field_dict["scanned_by_origin"] = scanned_by_origin
        if findings_unlisted is not UNSET:
            field_dict["findings_unlisted"] = findings_unlisted
        if promoted_unapproved is not UNSET:
            field_dict["promoted_unapproved"] = promoted_unapproved

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.rescan_report_by_rule_item import RescanReportByRuleItem
        from ..models.rescan_report_findings_item import RescanReportFindingsItem
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

        rescan_report = cls(
            scanner_version=scanner_version,
            scanned=scanned,
            findings=findings,
            by_rule=by_rule,
            next_after_seq=next_after_seq,
            recorded=recorded,
            held_unrescanned=held_unrescanned,
            rescan_due=rescan_due,
            scanned_by_origin=scanned_by_origin,
            findings_unlisted=findings_unlisted,
            promoted_unapproved=promoted_unapproved,
        )

        return rescan_report
