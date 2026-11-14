from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define

from ..models.recall_sources_searched_item_via import RecallSourcesSearchedItemVia

if TYPE_CHECKING:
    from ..models.recall_sources_searched_item_matched import (
        RecallSourcesSearchedItemMatched,
    )


T = TypeVar("T", bound="RecallSourcesSearchedItem")


@_attrs_define
class RecallSourcesSearchedItem:
    """
    Attributes:
        workspace_id (UUID):
        workspace_path (str):
        via (RecallSourcesSearchedItemVia):
        estimate (float): The share of the matching rows it holds, weighted by each word's rarity over every source
            searched or considered: the order selection reads above max_sources (d4.4).
        matched (RecallSourcesSearchedItemMatched):
    """

    workspace_id: UUID
    workspace_path: str
    via: RecallSourcesSearchedItemVia
    estimate: float
    matched: RecallSourcesSearchedItemMatched

    def to_dict(self) -> dict[str, Any]:
        workspace_id = str(self.workspace_id)

        workspace_path = self.workspace_path

        via = self.via.value

        estimate = self.estimate

        matched = self.matched.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "workspace_id": workspace_id,
                "workspace_path": workspace_path,
                "via": via,
                "estimate": estimate,
                "matched": matched,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.recall_sources_searched_item_matched import (
            RecallSourcesSearchedItemMatched,
        )

        d = dict(src_dict)
        workspace_id = UUID(d.pop("workspace_id"))

        workspace_path = d.pop("workspace_path")

        via = RecallSourcesSearchedItemVia(d.pop("via"))

        estimate = d.pop("estimate")

        matched = RecallSourcesSearchedItemMatched.from_dict(d.pop("matched"))

        recall_sources_searched_item = cls(
            workspace_id=workspace_id,
            workspace_path=workspace_path,
            via=via,
            estimate=estimate,
            matched=matched,
        )

        return recall_sources_searched_item
