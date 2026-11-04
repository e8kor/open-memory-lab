from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ReviewPolicy")


@_attrs_define
class ReviewPolicy:
    """The review policy in force, the one a transition's policy_snapshot records (ADR-0009 d3).

    Attributes:
        k (int | Unset):
        required_roles (list[str] | Unset):
    """

    k: int | Unset = UNSET
    required_roles: list[str] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        k = self.k

        required_roles: list[str] | Unset = UNSET
        if not isinstance(self.required_roles, Unset):
            required_roles = self.required_roles

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if k is not UNSET:
            field_dict["k"] = k
        if required_roles is not UNSET:
            field_dict["required_roles"] = required_roles

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        k = d.pop("k", UNSET)

        required_roles = cast(list[str], d.pop("required_roles", UNSET))

        review_policy = cls(
            k=k,
            required_roles=required_roles,
        )

        review_policy.additional_properties = d
        return review_policy

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
