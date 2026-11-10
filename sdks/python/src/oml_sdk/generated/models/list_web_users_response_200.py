from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.list_web_users_response_200_mode import ListWebUsersResponse200Mode

if TYPE_CHECKING:
    from ..models.list_web_users_response_200_deletions_item import (
        ListWebUsersResponse200DeletionsItem,
    )
    from ..models.web_user import WebUser


T = TypeVar("T", bound="ListWebUsersResponse200")


@_attrs_define
class ListWebUsersResponse200:
    """
    Attributes:
        mode (ListWebUsersResponse200Mode):
        address_limit_shared (bool):
        users (list[WebUser]):
        deletions (list[ListWebUsersResponse200DeletionsItem]):
    """

    mode: ListWebUsersResponse200Mode
    address_limit_shared: bool
    users: list[WebUser]
    deletions: list[ListWebUsersResponse200DeletionsItem]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        mode = self.mode.value

        address_limit_shared = self.address_limit_shared

        users = []
        for users_item_data in self.users:
            users_item = users_item_data.to_dict()
            users.append(users_item)

        deletions = []
        for deletions_item_data in self.deletions:
            deletions_item = deletions_item_data.to_dict()
            deletions.append(deletions_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "mode": mode,
                "address_limit_shared": address_limit_shared,
                "users": users,
                "deletions": deletions,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.list_web_users_response_200_deletions_item import (
            ListWebUsersResponse200DeletionsItem,
        )
        from ..models.web_user import WebUser

        d = dict(src_dict)
        mode = ListWebUsersResponse200Mode(d.pop("mode"))

        address_limit_shared = d.pop("address_limit_shared")

        users = []
        _users = d.pop("users")
        for users_item_data in _users:
            users_item = WebUser.from_dict(users_item_data)

            users.append(users_item)

        deletions = []
        _deletions = d.pop("deletions")
        for deletions_item_data in _deletions:
            deletions_item = ListWebUsersResponse200DeletionsItem.from_dict(
                deletions_item_data
            )

            deletions.append(deletions_item)

        list_web_users_response_200 = cls(
            mode=mode,
            address_limit_shared=address_limit_shared,
            users=users,
            deletions=deletions,
        )

        list_web_users_response_200.additional_properties = d
        return list_web_users_response_200

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
