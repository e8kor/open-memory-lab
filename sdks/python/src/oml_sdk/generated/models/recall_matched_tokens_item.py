from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="RecallMatchedTokensItem")


@_attrs_define
class RecallMatchedTokensItem:
    """
    Attributes:
        token (str): The word piece.
        weight (float): Its contribution to the sparse score: the query weight (the model's idf) times the row's weight.
        expanded (bool): true when the encoder added the piece and the row's own text does not hold it.
    """

    token: str
    weight: float
    expanded: bool

    def to_dict(self) -> dict[str, Any]:
        token = self.token

        weight = self.weight

        expanded = self.expanded

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "token": token,
                "weight": weight,
                "expanded": expanded,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        token = d.pop("token")

        weight = d.pop("weight")

        expanded = d.pop("expanded")

        recall_matched_tokens_item = cls(
            token=token,
            weight=weight,
            expanded=expanded,
        )

        return recall_matched_tokens_item
