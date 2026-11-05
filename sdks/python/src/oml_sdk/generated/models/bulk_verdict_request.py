from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define

from ..models.bulk_verdict_request_verdict import BulkVerdictRequestVerdict
from ..types import UNSET, Unset

T = TypeVar("T", bound="BulkVerdictRequest")


@_attrs_define
class BulkVerdictRequest:
    """
    Attributes:
        verdict (BulkVerdictRequestVerdict):
        artifact_ids (list[UUID]): The ids the reviewer was listed, never a filter (ADR-0055 d3).
        reason (str | Unset): Required to reject; journalled on each item.
    """

    verdict: BulkVerdictRequestVerdict
    artifact_ids: list[UUID]
    reason: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        verdict = self.verdict.value

        artifact_ids = []
        for artifact_ids_item_data in self.artifact_ids:
            artifact_ids_item = str(artifact_ids_item_data)
            artifact_ids.append(artifact_ids_item)

        reason = self.reason

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "verdict": verdict,
                "artifact_ids": artifact_ids,
            }
        )
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        verdict = BulkVerdictRequestVerdict(d.pop("verdict"))

        artifact_ids = []
        _artifact_ids = d.pop("artifact_ids")
        for artifact_ids_item_data in _artifact_ids:
            artifact_ids_item = UUID(artifact_ids_item_data)

            artifact_ids.append(artifact_ids_item)

        reason = d.pop("reason", UNSET)

        bulk_verdict_request = cls(
            verdict=verdict,
            artifact_ids=artifact_ids,
            reason=reason,
        )

        return bulk_verdict_request
