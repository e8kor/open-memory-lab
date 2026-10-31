from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ReviewNextBody")


@_attrs_define
class ReviewNextBody:
    """
    Attributes:
        workarea (str | Unset): A workarea, by its node id or its name. Resolved server-side before any check that reads
            state; an unknown name answers exactly what an unknown id answers. A derived path is neither, and is refused
            (request.validation).
    """

    workarea: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        workarea: str | Unset
        if isinstance(self.workarea, Unset):
            workarea = UNSET
        else:
            workarea = self.workarea

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if workarea is not UNSET:
            field_dict["workarea"] = workarea

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_workarea(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        workarea = _parse_workarea(d.pop("workarea", UNSET))

        review_next_body = cls(
            workarea=workarea,
        )

        review_next_body.additional_properties = d
        return review_next_body

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
