from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="WhoamiRef")


@_attrs_define
class WhoamiRef:
    """A row the calling key's record references, by its id and its name (ADR-0039 d15).

    Attributes:
        id (str):
        name (str):
    """

    id: str
    name: str

    def to_dict(self) -> dict[str, Any]:
        id = self.id

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

        name = d.pop("name")

        whoami_ref = cls(
            id=id,
            name=name,
        )

        return whoami_ref
