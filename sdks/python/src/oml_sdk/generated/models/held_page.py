from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.held_page_items_item import HeldPageItemsItem
    from ..models.held_page_lane_authors_type_0_item import HeldPageLaneAuthorsType0Item


T = TypeVar("T", bound="HeldPage")


@_attrs_define
class HeldPage:
    """
    Attributes:
        items (list[HeldPageItemsItem]):
        next_cursor (None | str):
        lane_authors (list[HeldPageLaneAuthorsType0Item] | None | Unset): With needs=person, the lane's rows per author
            key, in author key id order; null otherwise (ADR-0074 d19). Since v0.8.0.
    """

    items: list[HeldPageItemsItem]
    next_cursor: None | str
    lane_authors: list[HeldPageLaneAuthorsType0Item] | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        items = []
        for items_item_data in self.items:
            items_item = items_item_data.to_dict()
            items.append(items_item)

        next_cursor: None | str
        next_cursor = self.next_cursor

        lane_authors: list[dict[str, Any]] | None | Unset
        if isinstance(self.lane_authors, Unset):
            lane_authors = UNSET
        elif isinstance(self.lane_authors, list):
            lane_authors = []
            for lane_authors_type_0_item_data in self.lane_authors:
                lane_authors_type_0_item = lane_authors_type_0_item_data.to_dict()
                lane_authors.append(lane_authors_type_0_item)

        else:
            lane_authors = self.lane_authors

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "items": items,
                "next_cursor": next_cursor,
            }
        )
        if lane_authors is not UNSET:
            field_dict["lane_authors"] = lane_authors

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.held_page_items_item import HeldPageItemsItem
        from ..models.held_page_lane_authors_type_0_item import (
            HeldPageLaneAuthorsType0Item,
        )

        d = dict(src_dict)
        items = []
        _items = d.pop("items")
        for items_item_data in _items:
            items_item = HeldPageItemsItem.from_dict(items_item_data)

            items.append(items_item)

        def _parse_next_cursor(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        next_cursor = _parse_next_cursor(d.pop("next_cursor"))

        def _parse_lane_authors(
            data: object,
        ) -> list[HeldPageLaneAuthorsType0Item] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                lane_authors_type_0 = []
                _lane_authors_type_0 = data
                for lane_authors_type_0_item_data in _lane_authors_type_0:
                    lane_authors_type_0_item = HeldPageLaneAuthorsType0Item.from_dict(
                        lane_authors_type_0_item_data
                    )

                    lane_authors_type_0.append(lane_authors_type_0_item)

                return lane_authors_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[HeldPageLaneAuthorsType0Item] | None | Unset, data)

        lane_authors = _parse_lane_authors(d.pop("lane_authors", UNSET))

        held_page = cls(
            items=items,
            next_cursor=next_cursor,
            lane_authors=lane_authors,
        )

        held_page.additional_properties = d
        return held_page

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
