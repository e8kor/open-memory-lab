from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.memory import Memory


T = TypeVar("T", bound="HeldNamedItemsItem")


@_attrs_define
class HeldNamedItemsItem:
    """
    Attributes:
        id (UUID):
        needs_person (bool): true when only a person's reviewer key approves the row: it, or the version it replaces, is
            steering or a guardrail (ADR-0080 d16); false when item is null. Since v0.8.3.
        item (Memory | None): The row as the held listing serves its text, or null when it is no longer held or no
            longer the key's to judge.
    """

    id: UUID
    needs_person: bool
    item: Memory | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.memory import Memory

        id = str(self.id)

        needs_person = self.needs_person

        item: dict[str, Any] | None
        if isinstance(self.item, Memory):
            item = self.item.to_dict()
        else:
            item = self.item

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "needs_person": needs_person,
                "item": item,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.memory import Memory

        d = dict(src_dict)
        id = UUID(d.pop("id"))

        needs_person = d.pop("needs_person")

        def _parse_item(data: object) -> Memory | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                item_type_0 = Memory.from_dict(data)

                return item_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(Memory | None, data)

        item = _parse_item(d.pop("item"))

        held_named_items_item = cls(
            id=id,
            needs_person=needs_person,
            item=item,
        )

        held_named_items_item.additional_properties = d
        return held_named_items_item

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
