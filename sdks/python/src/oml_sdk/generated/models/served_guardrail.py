from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.severity import Severity

if TYPE_CHECKING:
    from ..models.served_guardrail_verdicts_item import ServedGuardrailVerdictsItem


T = TypeVar("T", bound="ServedGuardrail")


@_attrs_define
class ServedGuardrail:
    """
    Attributes:
        artifact_id (str):
        claim_key (str):
        version (int):
        workarea_id (str):
        workarea_path (None | str):
        depth (int): The node's depth from the root, the root at 0.
        severity (Severity): A guardrail's severity: never and must bind, should is the reviewed default, and it orders
            the set first; required on kind guardrail except a retirement, refused on every other kind (request.validation,
            details.field /severity; ADR-0037 d2, d10; since v0.5.1).
        text (str):
        rule (Any): Its oml-gr/1 rule, or null.
        tokens (int):
        promoted_at (datetime.datetime):
        verdicts (list[ServedGuardrailVerdictsItem]): Each approving verdict's reviewer attestation and time, never the
            reviewer's key (ADR-0025 d15).
        conflict (list[str]): The artifact_ids of the served rows of this claim_key with another severity; empty when
            none.
    """

    artifact_id: str
    claim_key: str
    version: int
    workarea_id: str
    workarea_path: None | str
    depth: int
    severity: Severity
    text: str
    rule: Any
    tokens: int
    promoted_at: datetime.datetime
    verdicts: list[ServedGuardrailVerdictsItem]
    conflict: list[str]

    def to_dict(self) -> dict[str, Any]:
        artifact_id = self.artifact_id

        claim_key = self.claim_key

        version = self.version

        workarea_id = self.workarea_id

        workarea_path: None | str
        workarea_path = self.workarea_path

        depth = self.depth

        severity = self.severity.value

        text = self.text

        rule = self.rule

        tokens = self.tokens

        promoted_at = self.promoted_at.isoformat()

        verdicts = []
        for verdicts_item_data in self.verdicts:
            verdicts_item = verdicts_item_data.to_dict()
            verdicts.append(verdicts_item)

        conflict = self.conflict

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "artifact_id": artifact_id,
                "claim_key": claim_key,
                "version": version,
                "workarea_id": workarea_id,
                "workarea_path": workarea_path,
                "depth": depth,
                "severity": severity,
                "text": text,
                "rule": rule,
                "tokens": tokens,
                "promoted_at": promoted_at,
                "verdicts": verdicts,
                "conflict": conflict,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.served_guardrail_verdicts_item import ServedGuardrailVerdictsItem

        d = dict(src_dict)
        artifact_id = d.pop("artifact_id")

        claim_key = d.pop("claim_key")

        version = d.pop("version")

        workarea_id = d.pop("workarea_id")

        def _parse_workarea_path(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        workarea_path = _parse_workarea_path(d.pop("workarea_path"))

        depth = d.pop("depth")

        severity = Severity(d.pop("severity"))

        text = d.pop("text")

        rule = d.pop("rule")

        tokens = d.pop("tokens")

        promoted_at = datetime.datetime.fromisoformat(d.pop("promoted_at"))

        verdicts = []
        _verdicts = d.pop("verdicts")
        for verdicts_item_data in _verdicts:
            verdicts_item = ServedGuardrailVerdictsItem.from_dict(verdicts_item_data)

            verdicts.append(verdicts_item)

        conflict = cast(list[str], d.pop("conflict"))

        served_guardrail = cls(
            artifact_id=artifact_id,
            claim_key=claim_key,
            version=version,
            workarea_id=workarea_id,
            workarea_path=workarea_path,
            depth=depth,
            severity=severity,
            text=text,
            rule=rule,
            tokens=tokens,
            promoted_at=promoted_at,
            verdicts=verdicts,
            conflict=conflict,
        )

        return served_guardrail
