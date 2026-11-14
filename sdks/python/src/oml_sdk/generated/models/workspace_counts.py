from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.workarea_counts_items_item import WorkareaCountsItemsItem
    from ..models.workarea_offers import WorkareaOffers


T = TypeVar("T", bound="WorkareaCounts")


@_attrs_define
class WorkareaCounts:
    """GET /v2/workareas/{workarea}/tree with counts=true (ADR-0045 d30; ADR-0070 d5).

    Attributes:
        items (list[WorkareaCountsItemsItem]):
        next_cursor (None):
        truncated (bool): The node or the row count stopped at 2000.
        offers (WorkareaOffers | Unset):
    """

    items: list[WorkareaCountsItemsItem]
    next_cursor: None
    truncated: bool
    offers: WorkareaOffers | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        items = []
        for items_item_data in self.items:
            items_item = items_item_data.to_dict()
            items.append(items_item)

        next_cursor = self.next_cursor

        truncated = self.truncated

        offers: dict[str, Any] | Unset = UNSET
        if not isinstance(self.offers, Unset):
            offers = self.offers.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "items": items,
                "next_cursor": next_cursor,
                "truncated": truncated,
            }
        )
        if offers is not UNSET:
            field_dict["offers"] = offers

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.workarea_counts_items_item import WorkareaCountsItemsItem
        from ..models.workarea_offers import WorkareaOffers

        d = dict(src_dict)
        items = []
        _items = d.pop("items")
        for items_item_data in _items:
            items_item = WorkareaCountsItemsItem.from_dict(items_item_data)

            items.append(items_item)

        next_cursor = d.pop("next_cursor")

        truncated = d.pop("truncated")

        _offers = d.pop("offers", UNSET)
        offers: WorkareaOffers | Unset
        if isinstance(_offers, Unset):
            offers = UNSET
        else:
            offers = WorkareaOffers.from_dict(_offers)

        workarea_counts = cls(
            items=items,
            next_cursor=next_cursor,
            truncated=truncated,
            offers=offers,
        )

        workarea_counts.additional_properties = d
        return workarea_counts

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
