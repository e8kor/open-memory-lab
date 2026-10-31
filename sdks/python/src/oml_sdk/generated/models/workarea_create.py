from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.crew_record import CrewRecord


T = TypeVar("T", bound="WorkareaCreate")


@_attrs_define
class WorkareaCreate:
    """
    Attributes:
        parent_id (str):
        name (str):
        crew (CrewRecord | Unset): Recorded, not operated — OML never contacts the external manager.
        prefix (str | Unset): Registers the node as a vocabulary workarea (ADR-0021): tenant-unique and permanent, never
            reused or reassigned. Conflict answers 409 vocab.prefix_conflict. A create carrying prefix requires crew — a
            vocabulary needs a named curator crew.
    """

    parent_id: str
    name: str
    crew: CrewRecord | Unset = UNSET
    prefix: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        parent_id = self.parent_id

        name = self.name

        crew: dict[str, Any] | Unset = UNSET
        if not isinstance(self.crew, Unset):
            crew = self.crew.to_dict()

        prefix = self.prefix

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "parent_id": parent_id,
                "name": name,
            }
        )
        if crew is not UNSET:
            field_dict["crew"] = crew
        if prefix is not UNSET:
            field_dict["prefix"] = prefix

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.crew_record import CrewRecord

        d = dict(src_dict)
        parent_id = d.pop("parent_id")

        name = d.pop("name")

        _crew = d.pop("crew", UNSET)
        crew: CrewRecord | Unset
        if isinstance(_crew, Unset):
            crew = UNSET
        else:
            crew = CrewRecord.from_dict(_crew)

        prefix = d.pop("prefix", UNSET)

        workarea_create = cls(
            parent_id=parent_id,
            name=name,
            crew=crew,
            prefix=prefix,
        )

        workarea_create.additional_properties = d
        return workarea_create

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
