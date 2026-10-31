from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ReassignRequest")


@_attrs_define
class ReassignRequest:
    """
    Attributes:
        workarea_id (str): A workarea, by its node id or its name. Resolved server-side before any check that reads
            state; an unknown name answers exactly what an unknown id answers. A derived path is neither, and is refused
            (request.validation).
        re_key (None | str | Unset): New claim_key when the move would collide at the destination.
        dry_run (bool | Unset):  Default: False.
    """

    workarea_id: str
    re_key: None | str | Unset = UNSET
    dry_run: bool | Unset = False
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        workarea_id: str
        workarea_id = self.workarea_id

        re_key: None | str | Unset
        if isinstance(self.re_key, Unset):
            re_key = UNSET
        else:
            re_key = self.re_key

        dry_run = self.dry_run

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "workarea_id": workarea_id,
            }
        )
        if re_key is not UNSET:
            field_dict["re_key"] = re_key
        if dry_run is not UNSET:
            field_dict["dry_run"] = dry_run

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_workarea_id(data: object) -> str:
            return cast(str, data)

        workarea_id = _parse_workarea_id(d.pop("workarea_id"))

        def _parse_re_key(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        re_key = _parse_re_key(d.pop("re_key", UNSET))

        dry_run = d.pop("dry_run", UNSET)

        reassign_request = cls(
            workarea_id=workarea_id,
            re_key=re_key,
            dry_run=dry_run,
        )

        reassign_request.additional_properties = d
        return reassign_request

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
