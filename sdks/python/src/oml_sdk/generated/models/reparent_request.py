from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ReparentRequest")


@_attrs_define
class ReparentRequest:
    """
    Attributes:
        new_parent_id (str): A workarea, by its node id or its name. Resolved server-side before any check that reads
            state; an unknown name answers exactly what an unknown id answers. A derived path is neither, and is refused
            (request.validation).
        dry_run (bool | Unset):  Default: False.
    """

    new_parent_id: str
    dry_run: bool | Unset = False
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        new_parent_id: str
        new_parent_id = self.new_parent_id

        dry_run = self.dry_run

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "new_parent_id": new_parent_id,
            }
        )
        if dry_run is not UNSET:
            field_dict["dry_run"] = dry_run

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_new_parent_id(data: object) -> str:
            return cast(str, data)

        new_parent_id = _parse_new_parent_id(d.pop("new_parent_id"))

        dry_run = d.pop("dry_run", UNSET)

        reparent_request = cls(
            new_parent_id=new_parent_id,
            dry_run=dry_run,
        )

        reparent_request.additional_properties = d
        return reparent_request

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
