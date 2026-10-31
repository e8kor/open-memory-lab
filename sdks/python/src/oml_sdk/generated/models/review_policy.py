from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ReviewPolicy")


@_attrs_define
class ReviewPolicy:
    """The per-workarea review policy object (journalled record; admission page owns it).

    Attributes:
        k (int | Unset):
        required_roles (list[str] | Unset):
        human_required_classes (list[str] | Unset):
        independence_axis (str | Unset):
        held_item_expiry (str | Unset):
    """

    k: int | Unset = UNSET
    required_roles: list[str] | Unset = UNSET
    human_required_classes: list[str] | Unset = UNSET
    independence_axis: str | Unset = UNSET
    held_item_expiry: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        k = self.k

        required_roles: list[str] | Unset = UNSET
        if not isinstance(self.required_roles, Unset):
            required_roles = self.required_roles

        human_required_classes: list[str] | Unset = UNSET
        if not isinstance(self.human_required_classes, Unset):
            human_required_classes = self.human_required_classes

        independence_axis = self.independence_axis

        held_item_expiry = self.held_item_expiry

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if k is not UNSET:
            field_dict["k"] = k
        if required_roles is not UNSET:
            field_dict["required_roles"] = required_roles
        if human_required_classes is not UNSET:
            field_dict["human_required_classes"] = human_required_classes
        if independence_axis is not UNSET:
            field_dict["independence_axis"] = independence_axis
        if held_item_expiry is not UNSET:
            field_dict["held_item_expiry"] = held_item_expiry

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        k = d.pop("k", UNSET)

        required_roles = cast(list[str], d.pop("required_roles", UNSET))

        human_required_classes = cast(list[str], d.pop("human_required_classes", UNSET))

        independence_axis = d.pop("independence_axis", UNSET)

        held_item_expiry = d.pop("held_item_expiry", UNSET)

        review_policy = cls(
            k=k,
            required_roles=required_roles,
            human_required_classes=human_required_classes,
            independence_axis=independence_axis,
            held_item_expiry=held_item_expiry,
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
