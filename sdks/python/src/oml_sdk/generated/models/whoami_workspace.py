from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="WhoamiWorkarea")


@_attrs_define
class WhoamiWorkarea:
    """The calling key's default workarea, by its id and its name, with the path derived from its ancestry, root first
    (ADR-0039 d15).

        Attributes:
            id (str):
            name (str):
            path (str): The names from the root to the node, joined by '.'; derived, never stored.
    """

    id: str
    name: str
    path: str

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        name = self.name

        path = self.path

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "id": id,
                "name": name,
                "path": path,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        path = d.pop("path")

        whoami_workarea = cls(
            id=id,
            name=name,
            path=path,
        )

        return whoami_workarea
