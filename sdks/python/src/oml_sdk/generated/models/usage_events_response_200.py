from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.recall_event import RecallEvent


T = TypeVar("T", bound="UsageEventsResponse200")


@_attrs_define
class UsageEventsResponse200:
    """
    Attributes:
        items (list[RecallEvent]):
        next_before (None | str):
    """

    items: list[RecallEvent]
    next_before: None | str

    def to_dict(self) -> dict[str, Any]:
        items = []
        for items_item_data in self.items:
            items_item = items_item_data.to_dict()
            items.append(items_item)

        next_before: None | str
        next_before = self.next_before

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "items": items,
                "next_before": next_before,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.recall_event import RecallEvent

        d = dict(src_dict)
        items = []
        _items = d.pop("items")
        for items_item_data in _items:
            items_item = RecallEvent.from_dict(items_item_data)

            items.append(items_item)

        def _parse_next_before(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        next_before = _parse_next_before(d.pop("next_before"))

        usage_events_response_200 = cls(
            items=items,
            next_before=next_before,
        )

        return usage_events_response_200
