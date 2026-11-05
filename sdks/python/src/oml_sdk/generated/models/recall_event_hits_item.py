from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define

T = TypeVar("T", bound="RecallEventHitsItem")


@_attrs_define
class RecallEventHitsItem:
    """
    Attributes:
        artifact_id (UUID):
        score (float):
    """

    artifact_id: UUID
    score: float

    def to_dict(self) -> dict[str, Any]:
        artifact_id = str(self.artifact_id)

        score = self.score

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "artifact_id": artifact_id,
                "score": score,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        artifact_id = UUID(d.pop("artifact_id"))

        score = d.pop("score")

        recall_event_hits_item = cls(
            artifact_id=artifact_id,
            score=score,
        )

        return recall_event_hits_item
