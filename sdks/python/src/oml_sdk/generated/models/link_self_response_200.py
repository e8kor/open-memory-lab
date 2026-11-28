from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="LinkSelfResponse200")


@_attrs_define
class LinkSelfResponse200:
    """
    Attributes:
        key_id (str):
        name (None | str):
        user (str):
        accepted (bool):
    """

    key_id: str
    name: None | str
    user: str
    accepted: bool

    def to_dict(self) -> dict[str, Any]:
        key_id = self.key_id

        name: None | str
        name = self.name

        user = self.user

        accepted = self.accepted

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "key_id": key_id,
                "name": name,
                "user": user,
                "accepted": accepted,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        key_id = d.pop("key_id")

        def _parse_name(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        name = _parse_name(d.pop("name"))

        user = d.pop("user")

        accepted = d.pop("accepted")

        link_self_response_200 = cls(
            key_id=key_id,
            name=name,
            user=user,
            accepted=accepted,
        )

        return link_self_response_200
