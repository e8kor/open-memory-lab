from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.listed_key_grants_item_covers import ListedKeyGrantsItemCovers
from ..models.role import Role
from ..types import UNSET, Unset

T = TypeVar("T", bound="ListedKeyGrantsItem")


@_attrs_define
class ListedKeyGrantsItem:
    """
    Attributes:
        role (Role):
        workspace_id (UUID):
        path (str):
        covers (ListedKeyGrantsItemCovers): The grant's coverage; a client reads an answer without it, from a server
            before v0.8.5, as subtree (ADR-0096 d8).
        create (bool | Unset): Whether the grant lets the key create child workspaces where it reaches; a client reads
            an answer without it, from a server before v0.8.6, as false (ADR-0097 d31).
    """

    role: Role
    workspace_id: UUID
    path: str
    covers: ListedKeyGrantsItemCovers
    create: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        role = self.role.value

        workspace_id = str(self.workspace_id)

        path = self.path

        covers = self.covers.value

        create = self.create

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "role": role,
                "workspace_id": workspace_id,
                "path": path,
                "covers": covers,
            }
        )
        if create is not UNSET:
            field_dict["create"] = create

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        role = Role(d.pop("role"))

        workspace_id = UUID(d.pop("workspace_id"))

        path = d.pop("path")

        covers = ListedKeyGrantsItemCovers(d.pop("covers"))

        create = d.pop("create", UNSET)

        listed_key_grants_item = cls(
            role=role,
            workspace_id=workspace_id,
            path=path,
            covers=covers,
            create=create,
        )

        listed_key_grants_item.additional_properties = d
        return listed_key_grants_item

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
