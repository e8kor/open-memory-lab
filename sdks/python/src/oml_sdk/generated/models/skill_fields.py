from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="SkillFields")


@_attrs_define
class SkillFields:
    """A skill's name and description, signed with the version (ADR-0080 d1).

    Attributes:
        name (str): A lowercase label: a letter first, then letters, digits and hyphens, ending in a letter or digit; no
            --; not guardrails, and not ending in -unreviewed (ADR-0080 d1, d10).
        description (str): One line.
    """

    name: str
    description: str

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        description = self.description

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "name": name,
                "description": description,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        name = d.pop("name")

        description = d.pop("description")

        skill_fields = cls(
            name=name,
            description=description,
        )

        return skill_fields
