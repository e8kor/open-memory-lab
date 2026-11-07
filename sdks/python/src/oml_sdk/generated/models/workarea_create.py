from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.workarea_create_mode import WorkareaCreateMode
from ..types import UNSET, Unset

T = TypeVar("T", bound="WorkareaCreate")


@_attrs_define
class WorkareaCreate:
    """
    Attributes:
        name (str): A new name, in a creation body: the creation grammar. A lowercase RFC 1035 label, 1-63 characters,
            that is not in the id form, not 32 hexadecimal digits and not the derived form, which only rows that predate
            names hold; a creation that sends that form is request.validation. Unique in its namespace, immutable, never
            released; compared byte for byte, never lowercased (docs/api/http-api.md, Names and ids; ADR-0026 d1, d2).
            References use NameRef.
        parent (str | Unset): A workarea, by its node id, its name or its qualified name. Resolved server-side before
            any check that reads state; an unknown name answers exactly what an unknown id answers. A user key resolves a
            bare name in its own user root's namespace, plus universe and its root's name; every other key in the shared
            namespace (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is universe,
            is neither, and is refused (request.validation).
        crew (str | Unset): A crew record, by its id or its name, resolved server-side like every reference (ADR-0026
            d4).
        mode (WorkareaCreateMode | Unset): in_progress, from a writer user key under a code parent only: the child is
            development and in progress from its first instant. Anywhere else the field is refused request.validation naming
            /mode (ADR-0064 d3).
    """

    name: str
    parent: str | Unset = UNSET
    crew: str | Unset = UNSET
    mode: WorkareaCreateMode | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        parent: str | Unset
        if isinstance(self.parent, Unset):
            parent = UNSET
        else:
            parent = self.parent

        crew: str | Unset
        if isinstance(self.crew, Unset):
            crew = UNSET
        else:
            crew = self.crew

        mode: str | Unset = UNSET
        if not isinstance(self.mode, Unset):
            mode = self.mode.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "name": name,
            }
        )
        if parent is not UNSET:
            field_dict["parent"] = parent
        if crew is not UNSET:
            field_dict["crew"] = crew
        if mode is not UNSET:
            field_dict["mode"] = mode

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        name = d.pop("name")

        def _parse_parent(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        parent = _parse_parent(d.pop("parent", UNSET))

        def _parse_crew(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        crew = _parse_crew(d.pop("crew", UNSET))

        _mode = d.pop("mode", UNSET)
        mode: WorkareaCreateMode | Unset
        if isinstance(_mode, Unset):
            mode = UNSET
        else:
            mode = WorkareaCreateMode(_mode)

        workarea_create = cls(
            name=name,
            parent=parent,
            crew=crew,
            mode=mode,
        )

        return workarea_create
