from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.memory import Memory
    from ..models.recall_matched import RecallMatched


T = TypeVar("T", bound="RecallResponseHitsItem")


@_attrs_define
class RecallResponseHitsItem:
    """
    Attributes:
        memory (Memory):
        score (float): The pipeline's score, comparable within one answer only; ties break by artifact_id ascending.
        matched (RecallMatched): Why a hit matched (ADR-0049 d6).
    """

    memory: Memory
    score: float
    matched: RecallMatched
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        memory = self.memory.to_dict()

        score = self.score

        matched = self.matched.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "memory": memory,
                "score": score,
                "matched": matched,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.memory import Memory
        from ..models.recall_matched import RecallMatched

        d = dict(src_dict)
        memory = Memory.from_dict(d.pop("memory"))

        score = d.pop("score")

        matched = RecallMatched.from_dict(d.pop("matched"))

        recall_response_hits_item = cls(
            memory=memory,
            score=score,
            matched=matched,
        )

        recall_response_hits_item.additional_properties = d
        return recall_response_hits_item

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
