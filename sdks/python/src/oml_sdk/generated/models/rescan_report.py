from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.rescan_report_by_rule_item import RescanReportByRuleItem
    from ..models.rescan_report_findings_item import RescanReportFindingsItem


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
    """

    scanner_version: None | str
    scanned: int
    findings: list[RescanReportFindingsItem]
    by_rule: list[RescanReportByRuleItem]
    next_after_seq: int | None

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

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "scanner_version": scanner_version,
                "scanned": scanned,
                "findings": findings,
                "by_rule": by_rule,
                "next_after_seq": next_after_seq,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.rescan_report_by_rule_item import RescanReportByRuleItem
        from ..models.rescan_report_findings_item import RescanReportFindingsItem

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

        rescan_report = cls(
            scanner_version=scanner_version,
            scanned=scanned,
            findings=findings,
            by_rule=by_rule,
            next_after_seq=next_after_seq,
        )

        return rescan_report
