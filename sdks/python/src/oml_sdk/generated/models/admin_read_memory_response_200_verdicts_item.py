from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="AdminReadMemoryResponse200VerdictsItem")


@_attrs_define
class AdminReadMemoryResponse200VerdictsItem:
    """
    Attributes:
        verdict (str | Unset):
        key (str | Unset):
        principal (None | str | Unset):
        reason (None | str | Unset):
        at (datetime.datetime | Unset):
    """

    verdict: str | Unset = UNSET
    key: str | Unset = UNSET
    principal: None | str | Unset = UNSET
    reason: None | str | Unset = UNSET
    at: datetime.datetime | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        verdict = self.verdict

        key = self.key

        principal: None | str | Unset
        if isinstance(self.principal, Unset):
            principal = UNSET
        else:
            principal = self.principal

        reason: None | str | Unset
        if isinstance(self.reason, Unset):
            reason = UNSET
        else:
            reason = self.reason

        at: str | Unset = UNSET
        if not isinstance(self.at, Unset):
            at = self.at.isoformat()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if verdict is not UNSET:
            field_dict["verdict"] = verdict
        if key is not UNSET:
            field_dict["key"] = key
        if principal is not UNSET:
            field_dict["principal"] = principal
        if reason is not UNSET:
            field_dict["reason"] = reason
        if at is not UNSET:
            field_dict["at"] = at

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        verdict = d.pop("verdict", UNSET)

        key = d.pop("key", UNSET)

        def _parse_principal(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        principal = _parse_principal(d.pop("principal", UNSET))

        def _parse_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        reason = _parse_reason(d.pop("reason", UNSET))

        _at = d.pop("at", UNSET)
        at: datetime.datetime | Unset
        if isinstance(_at, Unset):
            at = UNSET
        else:
            at = datetime.datetime.fromisoformat(_at)

        admin_read_memory_response_200_verdicts_item = cls(
            verdict=verdict,
            key=key,
            principal=principal,
            reason=reason,
            at=at,
        )

        admin_read_memory_response_200_verdicts_item.additional_properties = d
        return admin_read_memory_response_200_verdicts_item

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
