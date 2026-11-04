from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="WorkareaCreate")


@_attrs_define
class WorkareaCreate:
    """
    Attributes:
        parent (str): A workarea, by its node id or its name. Resolved server-side before any check that reads state; an
            unknown name answers exactly what an unknown id answers. A derived path is neither, and is refused
            (request.validation).
        name (str): A new name, in a creation body: the creation grammar. A lowercase RFC 1035 label, 1-63 characters,
            that is not in the id form, not 32 hexadecimal digits and not the derived form, which only rows that predate
            names hold; a creation that sends that form is request.validation. Unique in its namespace, immutable, never
            released; compared byte for byte, never lowercased (docs/api/http-api.md, Names and ids; ADR-0026 d1, d2).
            References use NameRef.
        crew (str | Unset): A crew record, by its id or its name, resolved server-side like every reference (ADR-0026
            d4).
    """

    parent: str
    name: str
    crew: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        parent: str
        parent = self.parent

        name = self.name

        crew: str | Unset
        if isinstance(self.crew, Unset):
            crew = UNSET
        else:
            crew = self.crew

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "parent": parent,
                "name": name,
            }
        )
        if crew is not UNSET:
            field_dict["crew"] = crew

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_parent(data: object) -> str:
            return cast(str, data)

        parent = _parse_parent(d.pop("parent"))

        name = d.pop("name")

        def _parse_crew(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        crew = _parse_crew(d.pop("crew", UNSET))

        workarea_create = cls(
            parent=parent,
            name=name,
            crew=crew,
        )

        return workarea_create
