from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.workarea_counts_items_item_mode import WorkareaCountsItemsItemMode

if TYPE_CHECKING:
    from ..models.workarea_counts_items_item_counts import WorkareaCountsItemsItemCounts


T = TypeVar("T", bound="WorkareaCountsItemsItem")


@_attrs_define
class WorkareaCountsItemsItem:
    """
    Attributes:
        id (UUID):
        name (str):
        parent_id (None | UUID):
        path (str):
        status (str):
        user_root (bool):
        mode (WorkareaCountsItemsItemMode):
        counts (WorkareaCountsItemsItemCounts):
    """

    id: UUID
    name: str
    parent_id: None | UUID
    path: str
    status: str
    user_root: bool
    mode: WorkareaCountsItemsItemMode
    counts: WorkareaCountsItemsItemCounts
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = str(self.id)

        name = self.name

        parent_id: None | str
        if isinstance(self.parent_id, UUID):
            parent_id = str(self.parent_id)
        else:
            parent_id = self.parent_id

        path = self.path

        status = self.status

        user_root = self.user_root

        mode = self.mode.value

        counts = self.counts.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "parent_id": parent_id,
                "path": path,
                "status": status,
                "user_root": user_root,
                "mode": mode,
                "counts": counts,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.workarea_counts_items_item_counts import (
            WorkareaCountsItemsItemCounts,
        )

        d = dict(src_dict)
        id = UUID(d.pop("id"))

        name = d.pop("name")

        def _parse_parent_id(data: object) -> None | UUID:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                parent_id_type_0 = UUID(data)

                return parent_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | UUID, data)

        parent_id = _parse_parent_id(d.pop("parent_id"))

        path = d.pop("path")

        status = d.pop("status")

        user_root = d.pop("user_root")

        mode = WorkareaCountsItemsItemMode(d.pop("mode"))

        counts = WorkareaCountsItemsItemCounts.from_dict(d.pop("counts"))

        workarea_counts_items_item = cls(
            id=id,
            name=name,
            parent_id=parent_id,
            path=path,
            status=status,
            user_root=user_root,
            mode=mode,
            counts=counts,
        )

        workarea_counts_items_item.additional_properties = d
        return workarea_counts_items_item

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
