from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="KeyLink")


@_attrs_define
class KeyLink:
    """A key and the user its link names, null when unlinked, and whether that user has yet to accept it (ADR-0085 d16, the
    operator's decision of 2026-11-19).

        Attributes:
            key_id (str):
            name (None | str):
            linked_user (None | str):
            pending (bool): True from a console admin's link until the named user accepts it; the key acts for no one
                meanwhile. Since v0.8.3.
    """

    key_id: str
    name: None | str
    linked_user: None | str
    pending: bool

    def to_dict(self) -> dict[str, Any]:
        key_id = self.key_id

        name: None | str
        name = self.name

        linked_user: None | str
        linked_user = self.linked_user

        pending = self.pending

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "key_id": key_id,
                "name": name,
                "linked_user": linked_user,
                "pending": pending,
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

        pending = d.pop("pending")

        key_link = cls(
            key_id=key_id,
            name=name,
            linked_user=linked_user,
            pending=pending,
        )

        return key_link
