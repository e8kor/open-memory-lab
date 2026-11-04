from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.workarea_status import WorkareaStatus
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.crew import Crew


T = TypeVar("T", bound="Workarea")


@_attrs_define
class Workarea:
    """
    Attributes:
        id (str):
        name (str): Unique in the tenant and never changed; universe for the root (ADR-0026 d1, d6).
        parent_id (None | str): Null only for the tenant universe root.
        path (str): Derived ltree path, the names from universe joined with "."; never the grant target, and never
            accepted as a reference.
        status (WorkareaStatus):
        crew (Crew | None | Unset): The crew record the workarea names, or null.
        created_at (datetime.datetime | Unset):
    """

    id: str
    name: str
    parent_id: None | str
    path: str
    status: WorkareaStatus
    crew: Crew | None | Unset = UNSET
    created_at: datetime.datetime | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.crew import Crew

        id = self.id

        name = self.name

        parent_id: None | str
        parent_id = self.parent_id

        path = self.path

        status = self.status.value

        crew: dict[str, Any] | None | Unset
        if isinstance(self.crew, Unset):
            crew = UNSET
        elif isinstance(self.crew, Crew):
            crew = self.crew.to_dict()
        else:
            crew = self.crew

        created_at: str | Unset = UNSET
        if not isinstance(self.created_at, Unset):
            created_at = self.created_at.isoformat()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "parent_id": parent_id,
                "path": path,
                "status": status,
            }
        )
        if crew is not UNSET:
            field_dict["crew"] = crew
        if created_at is not UNSET:
            field_dict["created_at"] = created_at

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.crew import Crew

        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        def _parse_parent_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        parent_id = _parse_parent_id(d.pop("parent_id"))

        path = d.pop("path")

        status = WorkareaStatus(d.pop("status"))

        def _parse_crew(data: object) -> Crew | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                crew_type_0 = Crew.from_dict(data)

                return crew_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(Crew | None | Unset, data)

        crew = _parse_crew(d.pop("crew", UNSET))

        _created_at = d.pop("created_at", UNSET)
        created_at: datetime.datetime | Unset
        if isinstance(_created_at, Unset):
            created_at = UNSET
        else:
            created_at = datetime.datetime.fromisoformat(_created_at)

        workarea = cls(
            id=id,
            name=name,
            parent_id=parent_id,
            path=path,
            status=status,
            crew=crew,
            created_at=created_at,
        )

        workarea.additional_properties = d
        return workarea

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
