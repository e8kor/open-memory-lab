from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="RecallSourcesSearchedItemMatched")


@_attrs_define
class RecallSourcesSearchedItemMatched:
    """
    Attributes:
        words (list[str]): For a federated source, the query words its rows hold; empty for the scope.
    """

    words: list[str]

    def to_dict(self) -> dict[str, Any]:
        words = self.words

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "words": words,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        words = cast(list[str], d.pop("words"))

        recall_sources_searched_item_matched = cls(
            words=words,
        )

        return recall_sources_searched_item_matched
