from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="Feedback")


@_attrs_define
class Feedback:
    """Which hits of this key's earlier recall answered its question and which did not (ADR-0054 d6). Checked before the
    read runs: at most 20 ids, each named once, a recall_id this key received in the last 24 hours, and only ids that
    recall served it; anything else is request.validation naming the field (/feedback/recall_id, /feedback/useful/0,
    ...) and the read is not run. A key's explicit labels past 1000 a UTC day answer limit.rate. A valid member changes
    nothing the read answers.

        Attributes:
            recall_id (UUID):
            useful (list[UUID] | Unset):
            not_useful (list[UUID] | Unset):
    """

    recall_id: UUID
    useful: list[UUID] | Unset = UNSET
    not_useful: list[UUID] | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        recall_id = str(self.recall_id)

        useful: list[str] | Unset = UNSET
        if not isinstance(self.useful, Unset):
            useful = []
            for useful_item_data in self.useful:
                useful_item = str(useful_item_data)
                useful.append(useful_item)

        not_useful: list[str] | Unset = UNSET
        if not isinstance(self.not_useful, Unset):
            not_useful = []
            for not_useful_item_data in self.not_useful:
                not_useful_item = str(not_useful_item_data)
                not_useful.append(not_useful_item)

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "recall_id": recall_id,
            }
        )
        if useful is not UNSET:
            field_dict["useful"] = useful
        if not_useful is not UNSET:
            field_dict["not_useful"] = not_useful

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        recall_id = UUID(d.pop("recall_id"))

        _useful = d.pop("useful", UNSET)
        useful: list[UUID] | Unset = UNSET
        if _useful is not UNSET:
            useful = []
            for useful_item_data in _useful:
                useful_item = UUID(useful_item_data)

                useful.append(useful_item)

        _not_useful = d.pop("not_useful", UNSET)
        not_useful: list[UUID] | Unset = UNSET
        if _not_useful is not UNSET:
            not_useful = []
            for not_useful_item_data in _not_useful:
                not_useful_item = UUID(not_useful_item_data)

                not_useful.append(not_useful_item)

        feedback = cls(
            recall_id=recall_id,
            useful=useful,
            not_useful=not_useful,
        )

        return feedback
