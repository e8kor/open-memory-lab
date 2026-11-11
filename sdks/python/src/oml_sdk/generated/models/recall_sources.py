from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.recall_sources_mode import RecallSourcesMode

if TYPE_CHECKING:
    from ..models.recall_sources_cut import RecallSourcesCut
    from ..models.recall_sources_searched_item import RecallSourcesSearchedItem


T = TypeVar("T", bound="RecallSources")


@_attrs_define
class RecallSources:
    """Under sources wide or a list: what was searched and why (ADR-0072 d8). Every name and number is of a workarea the
    key reads (d6).

        Attributes:
            mode (RecallSourcesMode):
            considered (int): Under wide, the trusted workareas the key reads outside the scope that accept federated reads;
                for a list, the names listed.
            searched (list[RecallSourcesSearchedItem]): The scope's nodes first, then the federated sources in selection
                order; for a list, the listed nodes in the order sent.
            cut (RecallSourcesCut): The workareas holding a query word that wide did not search because max_sources was
                reached; their statistics still scored every hit.
    """

    mode: RecallSourcesMode
    considered: int
    searched: list[RecallSourcesSearchedItem]
    cut: RecallSourcesCut

    def to_dict(self) -> dict[str, Any]:
        mode = self.mode.value

        considered = self.considered

        searched = []
        for searched_item_data in self.searched:
            searched_item = searched_item_data.to_dict()
            searched.append(searched_item)

        cut = self.cut.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "mode": mode,
                "considered": considered,
                "searched": searched,
                "cut": cut,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.recall_sources_cut import RecallSourcesCut
        from ..models.recall_sources_searched_item import RecallSourcesSearchedItem

        d = dict(src_dict)
        mode = RecallSourcesMode(d.pop("mode"))

        considered = d.pop("considered")

        searched = []
        _searched = d.pop("searched")
        for searched_item_data in _searched:
            searched_item = RecallSourcesSearchedItem.from_dict(searched_item_data)

            searched.append(searched_item)

        cut = RecallSourcesCut.from_dict(d.pop("cut"))

        recall_sources = cls(
            mode=mode,
            considered=considered,
            searched=searched,
            cut=cut,
        )

        return recall_sources
