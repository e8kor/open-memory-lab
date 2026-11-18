from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.memory_preview_passages_item import MemoryPreviewPassagesItem
    from ..models.memory_preview_refusal_type_0 import MemoryPreviewRefusalType0
    from ..models.memory_preview_rule import MemoryPreviewRule
    from ..models.memory_preview_set_after import MemoryPreviewSetAfter
    from ..models.memory_preview_shadows_item import MemoryPreviewShadowsItem
    from ..models.scan_finding import ScanFinding


T = TypeVar("T", bound="MemoryPreview")


@_attrs_define
class MemoryPreview:
    """What capture would decide for a previewed body (ADR-0082 d5, d10). The facts are those read before any refusal; a
    refusal of the body's shape answers none.

        Attributes:
            would_capture (bool):
            refusal (MemoryPreviewRefusalType0 | None): The refusal capture would answer, in the error object's shape; null
                when it would capture.
            findings (list[ScanFinding]): The scan's findings, as a held version's list carries them; empty when the scan
                did not run or found nothing.
            needs_person (bool): Whether a person's verdict is needed: an instruction kind, or a successor of one (ADR-0074
                d24; ADR-0080 d16).
            body_bytes (int | None): The NFC body's UTF-8 bytes.
            budget_tokens (int | None): The body's budget tokens under oml-tc/1.
            rule (MemoryPreviewRule | Unset): A guardrail's rule in its canonical oml-gr/1 form.
            offered_as (str | Unset): A skill's prompt name (ADR-0080 d5).
            shadows (list[MemoryPreviewShadowsItem] | Unset): A skill's same-named skills along its chain that the key
                reads: one on an ancestor it shadows, one on a descendant that shadows it there (ADR-0080 d5).
            set_after (MemoryPreviewSetAfter | Unset): A guardrail's set size at its workspace once the version is approved,
                the approved rules an agent there loads, against the set cap (ADR-0037 d7).
            passages (list[MemoryPreviewPassagesItem] | Unset): A document's passages as capture would cut them, each by its
                fragment and its lines, as a recall hit names its passage; absent for any other kind and a retirement (ADR-0081
                d3; ADR-0082 d5).
    """

    would_capture: bool
    refusal: MemoryPreviewRefusalType0 | None
    findings: list[ScanFinding]
    needs_person: bool
    body_bytes: int | None
    budget_tokens: int | None
    rule: MemoryPreviewRule | Unset = UNSET
    offered_as: str | Unset = UNSET
    shadows: list[MemoryPreviewShadowsItem] | Unset = UNSET
    set_after: MemoryPreviewSetAfter | Unset = UNSET
    passages: list[MemoryPreviewPassagesItem] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.memory_preview_refusal_type_0 import MemoryPreviewRefusalType0

        would_capture = self.would_capture

        refusal: dict[str, Any] | None
        if isinstance(self.refusal, MemoryPreviewRefusalType0):
            refusal = self.refusal.to_dict()
        else:
            refusal = self.refusal

        findings = []
        for findings_item_data in self.findings:
            findings_item = findings_item_data.to_dict()
            findings.append(findings_item)

        needs_person = self.needs_person

        body_bytes: int | None
        body_bytes = self.body_bytes

        budget_tokens: int | None
        budget_tokens = self.budget_tokens

        rule: dict[str, Any] | Unset = UNSET
        if not isinstance(self.rule, Unset):
            rule = self.rule.to_dict()

        offered_as = self.offered_as

        shadows: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.shadows, Unset):
            shadows = []
            for shadows_item_data in self.shadows:
                shadows_item = shadows_item_data.to_dict()
                shadows.append(shadows_item)

        set_after: dict[str, Any] | Unset = UNSET
        if not isinstance(self.set_after, Unset):
            set_after = self.set_after.to_dict()

        passages: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.passages, Unset):
            passages = []
            for passages_item_data in self.passages:
                passages_item = passages_item_data.to_dict()
                passages.append(passages_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "would_capture": would_capture,
                "refusal": refusal,
                "findings": findings,
                "needs_person": needs_person,
                "body_bytes": body_bytes,
                "budget_tokens": budget_tokens,
            }
        )
        if rule is not UNSET:
            field_dict["rule"] = rule
        if offered_as is not UNSET:
            field_dict["offered_as"] = offered_as
        if shadows is not UNSET:
            field_dict["shadows"] = shadows
        if set_after is not UNSET:
            field_dict["set_after"] = set_after
        if passages is not UNSET:
            field_dict["passages"] = passages

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.memory_preview_passages_item import MemoryPreviewPassagesItem
        from ..models.memory_preview_refusal_type_0 import MemoryPreviewRefusalType0
        from ..models.memory_preview_rule import MemoryPreviewRule
        from ..models.memory_preview_set_after import MemoryPreviewSetAfter
        from ..models.memory_preview_shadows_item import MemoryPreviewShadowsItem
        from ..models.scan_finding import ScanFinding

        d = dict(src_dict)
        would_capture = d.pop("would_capture")

        def _parse_refusal(data: object) -> MemoryPreviewRefusalType0 | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                refusal_type_0 = MemoryPreviewRefusalType0.from_dict(data)

                return refusal_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(MemoryPreviewRefusalType0 | None, data)

        refusal = _parse_refusal(d.pop("refusal"))

        findings = []
        _findings = d.pop("findings")
        for findings_item_data in _findings:
            findings_item = ScanFinding.from_dict(findings_item_data)

            findings.append(findings_item)

        needs_person = d.pop("needs_person")

        def _parse_body_bytes(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        body_bytes = _parse_body_bytes(d.pop("body_bytes"))

        def _parse_budget_tokens(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        budget_tokens = _parse_budget_tokens(d.pop("budget_tokens"))

        _rule = d.pop("rule", UNSET)
        rule: MemoryPreviewRule | Unset
        if isinstance(_rule, Unset):
            rule = UNSET
        else:
            rule = MemoryPreviewRule.from_dict(_rule)

        offered_as = d.pop("offered_as", UNSET)

        _shadows = d.pop("shadows", UNSET)
        shadows: list[MemoryPreviewShadowsItem] | Unset = UNSET
        if _shadows is not UNSET:
            shadows = []
            for shadows_item_data in _shadows:
                shadows_item = MemoryPreviewShadowsItem.from_dict(shadows_item_data)

                shadows.append(shadows_item)

        _set_after = d.pop("set_after", UNSET)
        set_after: MemoryPreviewSetAfter | Unset
        if isinstance(_set_after, Unset):
            set_after = UNSET
        else:
            set_after = MemoryPreviewSetAfter.from_dict(_set_after)

        _passages = d.pop("passages", UNSET)
        passages: list[MemoryPreviewPassagesItem] | Unset = UNSET
        if _passages is not UNSET:
            passages = []
            for passages_item_data in _passages:
                passages_item = MemoryPreviewPassagesItem.from_dict(passages_item_data)

                passages.append(passages_item)

        memory_preview = cls(
            would_capture=would_capture,
            refusal=refusal,
            findings=findings,
            needs_person=needs_person,
            body_bytes=body_bytes,
            budget_tokens=budget_tokens,
            rule=rule,
            offered_as=offered_as,
            shadows=shadows,
            set_after=set_after,
            passages=passages,
        )

        memory_preview.additional_properties = d
        return memory_preview

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
