from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="KeyLink")


@_attrs_define
class KeyLink:
    """A key and the user the console acts with it for, null when unlinked (ADR-0085 d16).

    Attributes:
        key_id (str):
        name (None | str):
        linked_user (None | str):
    """

    key_id: str
    name: None | str
    linked_user: None | str

    def to_dict(self) -> dict[str, Any]:
        key_id = self.key_id

        name: None | str
        name = self.name

        linked_user: None | str
        linked_user = self.linked_user

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "key_id": key_id,
                "name": name,
                "linked_user": linked_user,
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

        def _parse_linked_user(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        linked_user = _parse_linked_user(d.pop("linked_user"))

        key_link = cls(
            key_id=key_id,
            name=name,
            linked_user=linked_user,
        )

        return key_link
