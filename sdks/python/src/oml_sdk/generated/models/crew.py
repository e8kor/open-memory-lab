from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="Crew")


@_attrs_define
class Crew:
    """A named crew record — recorded, not operated; OML never contacts the external manager
    (docs/architecture/workareas.md, Crews; ADR-0026 d8).

        Attributes:
            crew_id (str):
            name (str): Unique in the tenant and never changed.
            charter_uri (None | str | Unset):
            external_manager (None | str | Unset):
            created_at (datetime.datetime | Unset):
    """

    crew_id: str
    name: str
    charter_uri: None | str | Unset = UNSET
    external_manager: None | str | Unset = UNSET
    created_at: datetime.datetime | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        crew_id = self.crew_id

        name = self.name

        charter_uri: None | str | Unset
        if isinstance(self.charter_uri, Unset):
            charter_uri = UNSET
        else:
            charter_uri = self.charter_uri

        external_manager: None | str | Unset
        if isinstance(self.external_manager, Unset):
            external_manager = UNSET
        else:
            external_manager = self.external_manager

        created_at: str | Unset = UNSET
        if not isinstance(self.created_at, Unset):
            created_at = self.created_at.isoformat()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "crew_id": crew_id,
                "name": name,
            }
        )
        if charter_uri is not UNSET:
            field_dict["charter_uri"] = charter_uri
        if external_manager is not UNSET:
            field_dict["external_manager"] = external_manager
        if created_at is not UNSET:
            field_dict["created_at"] = created_at

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        crew_id = d.pop("crew_id")

        name = d.pop("name")

        def _parse_charter_uri(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        charter_uri = _parse_charter_uri(d.pop("charter_uri", UNSET))

        def _parse_external_manager(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        external_manager = _parse_external_manager(d.pop("external_manager", UNSET))

        _created_at = d.pop("created_at", UNSET)
        created_at: datetime.datetime | Unset
        if isinstance(_created_at, Unset):
            created_at = UNSET
        else:
            created_at = datetime.datetime.fromisoformat(_created_at)

        crew = cls(
            crew_id=crew_id,
            name=name,
            charter_uri=charter_uri,
            external_manager=external_manager,
            created_at=created_at,
        )

        crew.additional_properties = d
        return crew

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
