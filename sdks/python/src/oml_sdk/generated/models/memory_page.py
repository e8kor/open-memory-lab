from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.memory import Memory
    from ..models.memory_page_counts import MemoryPageCounts


T = TypeVar("T", bound="MemoryPage")


@_attrs_define
class MemoryPage:
    """
    Attributes:
        items (list[Memory]):
        next_cursor (None | str):
        counts (MemoryPageCounts | Unset): The catalog's first page only: per kind, the entries every filter but kinds
            admits, over exactly the rows the page's predicate admits (ADR-0092 d6).
    """

    items: list[Memory]
    next_cursor: None | str
    counts: MemoryPageCounts | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        items = []
        for items_item_data in self.items:
            items_item = items_item_data.to_dict()
            items.append(items_item)

        next_cursor: None | str
        next_cursor = self.next_cursor

        counts: dict[str, Any] | Unset = UNSET
        if not isinstance(self.counts, Unset):
            counts = self.counts.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "items": items,
                "next_cursor": next_cursor,
            }
        )
        if counts is not UNSET:
            field_dict["counts"] = counts

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.memory import Memory
        from ..models.memory_page_counts import MemoryPageCounts

        d = dict(src_dict)
        items = []
        _items = d.pop("items")
        for items_item_data in _items:
            items_item = Memory.from_dict(items_item_data)

            items.append(items_item)

        def _parse_next_cursor(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        next_cursor = _parse_next_cursor(d.pop("next_cursor"))

        _counts = d.pop("counts", UNSET)
        counts: MemoryPageCounts | Unset
        if isinstance(_counts, Unset):
            counts = UNSET
        else:
            counts = MemoryPageCounts.from_dict(_counts)

        memory_page = cls(
            items=items,
            next_cursor=next_cursor,
            counts=counts,
        )

        memory_page.additional_properties = d
        return memory_page

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
