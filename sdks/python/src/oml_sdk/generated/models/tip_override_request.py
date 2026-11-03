from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="TipOverrideRequest")


@_attrs_define
class TipOverrideRequest:
    """
    Attributes:
        workarea_id (str): A workarea, by its node id or its name. Resolved server-side before any check that reads
            state; an unknown name answers exactly what an unknown id answers. A derived path is neither, and is refused
            (request.validation).
        claim_key (str):
        to_version (int):
        reason (str):
    """

    workarea_id: str
    claim_key: str
    to_version: int
    reason: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        workarea_id: str
        workarea_id = self.workarea_id

        claim_key = self.claim_key

        to_version = self.to_version

        reason = self.reason

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "workarea_id": workarea_id,
                "claim_key": claim_key,
                "to_version": to_version,
                "reason": reason,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_workarea_id(data: object) -> str:
            return cast(str, data)

        workarea_id = _parse_workarea_id(d.pop("workarea_id"))

        claim_key = d.pop("claim_key")

        to_version = d.pop("to_version")

        reason = d.pop("reason")

        tip_override_request = cls(
            workarea_id=workarea_id,
            claim_key=claim_key,
            to_version=to_version,
            reason=reason,
        )

        tip_override_request.additional_properties = d
        return tip_override_request

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
