from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.workarea_offers_mode_change_type_0_to import (
    WorkareaOffersModeChangeType0To,
)
from ..types import UNSET, Unset

T = TypeVar("T", bound="WorkareaOffersModeChangeType0")


@_attrs_define
class WorkareaOffersModeChangeType0:
    """Putting the workarea in progress, where offered; null otherwise.

    Attributes:
        to (WorkareaOffersModeChangeType0To | Unset):
    """

    to: WorkareaOffersModeChangeType0To | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        to: str | Unset = UNSET
        if not isinstance(self.to, Unset):
            to = self.to.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if to is not UNSET:
            field_dict["to"] = to

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        _to = d.pop("to", UNSET)
        to: WorkareaOffersModeChangeType0To | Unset
        if isinstance(_to, Unset):
            to = UNSET
        else:
            to = WorkareaOffersModeChangeType0To(_to)

        workarea_offers_mode_change_type_0 = cls(
            to=to,
        )

        workarea_offers_mode_change_type_0.additional_properties = d
        return workarea_offers_mode_change_type_0

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
