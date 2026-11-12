from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.scan_finding_found_by import ScanFindingFoundBy
from ..models.scan_finding_verdict import ScanFindingVerdict

T = TypeVar("T", bound="ScanFinding")


@_attrs_define
class ScanFinding:
    """One finding of a version's findings list (ADR-0074 d23): where the scan fired and why, never the matched text
    (ADR-0065 d8).

        Attributes:
            class_ (str): The threat class, injection.override.
            rule (str): The rule id, ignore-instructions.
            verdict (ScanFindingVerdict): refuse appears only on a finding a recording rescan made on a version already
                stored (ADR-0074 d14).
            rule_set (None | str): The rule set it was found under, oml-scan/1.2.
            field (None | str): /body, /title, or /body/ and a decoded view's path; null on a finding migrated from the
                single stored finding.
            line (int | None): The 1-based line of the field the match starts on.
            via (None | str): The view the rule read: raw, clean, spaced or letters (ADR-0074 d6).
            span (list[int] | None): The match's byte range, start and end, in that view.
            found_by (ScanFindingFoundBy): Who recorded it.
            event_seq (int | None): The seq of the journal event that recorded it.
            migrated_from (None | str): On a finding migrated from the version's single scan_finding, the text that held.
    """

    class_: str
    rule: str
    verdict: ScanFindingVerdict
    rule_set: None | str
    field: None | str
    line: int | None
    via: None | str
    span: list[int] | None
    found_by: ScanFindingFoundBy
    event_seq: int | None
    migrated_from: None | str

    def to_dict(self) -> dict[str, Any]:
        class_ = self.class_

        rule = self.rule

        verdict = self.verdict.value

        rule_set: None | str
        rule_set = self.rule_set

        field: None | str
        field = self.field

        line: int | None
        line = self.line

        via: None | str
        via = self.via

        span: list[int] | None
        if isinstance(self.span, list):
            span = self.span

        else:
            span = self.span

        found_by = self.found_by.value

        event_seq: int | None
        event_seq = self.event_seq

        migrated_from: None | str
        migrated_from = self.migrated_from

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "class": class_,
                "rule": rule,
                "verdict": verdict,
                "rule_set": rule_set,
                "field": field,
                "line": line,
                "via": via,
                "span": span,
                "found_by": found_by,
                "event_seq": event_seq,
                "migrated_from": migrated_from,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        class_ = d.pop("class")

        rule = d.pop("rule")

        verdict = ScanFindingVerdict(d.pop("verdict"))

        def _parse_rule_set(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        rule_set = _parse_rule_set(d.pop("rule_set"))

        def _parse_field(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        field = _parse_field(d.pop("field"))

        def _parse_line(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        line = _parse_line(d.pop("line"))

        def _parse_via(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        via = _parse_via(d.pop("via"))

        def _parse_span(data: object) -> list[int] | None:
            if data is None:
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                span_type_0 = cast(list[int], data)

                return span_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[int] | None, data)

        span = _parse_span(d.pop("span"))

        found_by = ScanFindingFoundBy(d.pop("found_by"))

        def _parse_event_seq(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        event_seq = _parse_event_seq(d.pop("event_seq"))

        def _parse_migrated_from(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        migrated_from = _parse_migrated_from(d.pop("migrated_from"))

        scan_finding = cls(
            class_=class_,
            rule=rule,
            verdict=verdict,
            rule_set=rule_set,
            field=field,
            line=line,
            via=via,
            span=span,
            found_by=found_by,
            event_seq=event_seq,
            migrated_from=migrated_from,
        )

        return scan_finding
