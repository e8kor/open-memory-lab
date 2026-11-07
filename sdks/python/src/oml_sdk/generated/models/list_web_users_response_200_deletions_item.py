from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="ListWebUsersResponse200DeletionsItem")


@_attrs_define
class ListWebUsersResponse200DeletionsItem:
    """
    Attributes:
        user (str):
        requested_at (datetime.datetime):
    """

    user: str
    requested_at: datetime.datetime
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        user = self.user

        requested_at = self.requested_at.isoformat()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "user": user,
                "requested_at": requested_at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        user = d.pop("user")

        requested_at = datetime.datetime.fromisoformat(d.pop("requested_at"))

        list_web_users_response_200_deletions_item = cls(
            user=user,
            requested_at=requested_at,
        )

        list_web_users_response_200_deletions_item.additional_properties = d
        return list_web_users_response_200_deletions_item

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
