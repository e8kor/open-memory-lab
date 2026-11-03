from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.grant_role import GrantRole
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.grant_deny_item import GrantDenyItem


T = TypeVar("T", bound="Grant")


@_attrs_define
class Grant:
    """A stored grant, as echoed at issuance and listed on a key.

    Attributes:
        grant_id (str):
        workarea_node_id (str): Node id, never a path.
        role (GrantRole):
        epoch (int): The journal seq of this grant's grant.issued event.
        deny (list[GrantDenyItem] | Unset):
    """

    grant_id: str
    workarea_node_id: str
    role: GrantRole
    epoch: int
    deny: list[GrantDenyItem] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        grant_id = self.grant_id

        workarea_node_id = self.workarea_node_id

        role = self.role.value

        epoch = self.epoch

        deny: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.deny, Unset):
            deny = []
            for deny_item_data in self.deny:
                deny_item = deny_item_data.to_dict()
                deny.append(deny_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "grant_id": grant_id,
                "workarea_node_id": workarea_node_id,
                "role": role,
                "epoch": epoch,
            }
        )
        if deny is not UNSET:
            field_dict["deny"] = deny

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.grant_deny_item import GrantDenyItem

        d = dict(src_dict)
        grant_id = d.pop("grant_id")

        workarea_node_id = d.pop("workarea_node_id")

        role = GrantRole(d.pop("role"))

        epoch = d.pop("epoch")

        _deny = d.pop("deny", UNSET)
        deny: list[GrantDenyItem] | Unset = UNSET
        if _deny is not UNSET:
            deny = []
            for deny_item_data in _deny:
                deny_item = GrantDenyItem.from_dict(deny_item_data)

                deny.append(deny_item)

        grant = cls(
            grant_id=grant_id,
            workarea_node_id=workarea_node_id,
            role=role,
            epoch=epoch,
            deny=deny,
        )

        grant.additional_properties = d
        return grant

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
