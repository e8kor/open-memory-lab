from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="WorkareaCreate")


@_attrs_define
class WorkareaCreate:
    """
    Attributes:
        parent_id (str): A workarea, by its node id or its name. Resolved server-side before any check that reads state;
            an unknown name answers exactly what an unknown id answers. A derived path is neither, and is refused
            (request.validation).
        name (str): A new name, in a creation body: the creation grammar. A lowercase RFC 1035 label, 1-63 characters,
            that is not in the id form, not 32 hexadecimal digits and not the derived form, which only rows that predate
            names hold; a creation that sends that form is request.validation. Unique in its namespace, immutable, never
            released; compared byte for byte, never lowercased (docs/api/http-api.md, Names and ids; ADR-0026 d1, d2).
            References use NameRef.
        crew (str | Unset): A crew record, by its id or its name, resolved server-side like every reference (ADR-0026
            d4).
        prefix (str | Unset): Registers the node as a vocabulary workarea (ADR-0021): tenant-unique and permanent, never
            reused or reassigned. Conflict answers 409 vocab.prefix_conflict. A create carrying prefix requires crew — a
            vocabulary needs a named curator crew record.
    """

    parent_id: str
    name: str
    crew: str | Unset = UNSET
    prefix: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        parent_id: str
        parent_id = self.parent_id

        name = self.name

        crew: str | Unset
        if isinstance(self.crew, Unset):
            crew = UNSET
        else:
            crew = self.crew

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
        d = dict(src_dict)

        def _parse_parent_id(data: object) -> str:
            return cast(str, data)

        parent_id = _parse_parent_id(d.pop("parent_id"))

        name = d.pop("name")

        def _parse_crew(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        crew = _parse_crew(d.pop("crew", UNSET))

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
