from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="CrewUpdate")


@_attrs_define
class CrewUpdate:
    """At least one field; the name is not updatable.

    Attributes:
        charter_uri (str | Unset):
        external_manager (str | Unset):
    """

    charter_uri: str | Unset = UNSET
    external_manager: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        charter_uri = self.charter_uri

        external_manager = self.external_manager

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if charter_uri is not UNSET:
            field_dict["charter_uri"] = charter_uri
        if external_manager is not UNSET:
            field_dict["external_manager"] = external_manager

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        charter_uri = d.pop("charter_uri", UNSET)

        external_manager = d.pop("external_manager", UNSET)

        crew_update = cls(
            charter_uri=charter_uri,
            external_manager=external_manager,
        )

        return crew_update
