from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="WorkareaCrewUpdate")


@_attrs_define
class WorkareaCrewUpdate:
    """
    Attributes:
        crew (str): A crew record, by its id or its name, resolved server-side like every reference (ADR-0026 d4).
    """

    crew: str

    def to_dict(self) -> dict[str, Any]:
        crew: str
        crew = self.crew

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "crew": crew,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_crew(data: object) -> str:
            return cast(str, data)

        crew = _parse_crew(d.pop("crew"))

        workarea_crew_update = cls(
            crew=crew,
        )

        return workarea_crew_update
