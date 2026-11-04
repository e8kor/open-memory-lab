from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.recall_matched_via_item import RecallMatchedViaItem

T = TypeVar("T", bound="RecallMatched")


@_attrs_define
class RecallMatched:
    """Why a hit matched (ADR-0049 d6).

    Attributes:
        terms (list[str]): The query words the row holds, as the pipeline reads words.
        of (int): How many words the query had.
        via (list[RecallMatchedViaItem]): The retrievers whose lists held the row.
    """

    terms: list[str]
    of: int
    via: list[RecallMatchedViaItem]

    def to_dict(self) -> dict[str, Any]:
        terms = self.terms

        of = self.of

        via = []
        for via_item_data in self.via:
            via_item = via_item_data.value
            via.append(via_item)

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "terms": terms,
                "of": of,
                "via": via,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        terms = cast(list[str], d.pop("terms"))

        of = d.pop("of")

        via = []
        _via = d.pop("via")
        for via_item_data in _via:
            via_item = RecallMatchedViaItem(via_item_data)

            via.append(via_item)

        recall_matched = cls(
            terms=terms,
            of=of,
            via=via,
        )

        return recall_matched
