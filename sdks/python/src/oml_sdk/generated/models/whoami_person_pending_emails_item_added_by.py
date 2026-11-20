from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="WhoamiPersonPendingEmailsItemAddedBy")


@_attrs_define
class WhoamiPersonPendingEmailsItemAddedBy:
    """
    Attributes:
        id (str):
        name (None | str): The key's name; null for a key issued before names (ADR-0026 d11).
    """

    id: str
    name: None | str

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        name: None | str
        name = self.name

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "id": id,
                "name": name,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        id = d.pop("id")

        def _parse_name(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        name = _parse_name(d.pop("name"))

        whoami_person_pending_emails_item_added_by = cls(
            id=id,
            name=name,
        )

        return whoami_person_pending_emails_item_added_by
