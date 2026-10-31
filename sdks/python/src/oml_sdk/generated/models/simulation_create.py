from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.simulation_create_audience_item import SimulationCreateAudienceItem


T = TypeVar("T", bound="SimulationCreate")


@_attrs_define
class SimulationCreate:
    """
    Attributes:
        audience (list[SimulationCreateAudienceItem]):
        label (str | Unset):
    """

    audience: list[SimulationCreateAudienceItem]
    label: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        audience = []
        for audience_item_data in self.audience:
            audience_item = audience_item_data.to_dict()
            audience.append(audience_item)

        label = self.label

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "audience": audience,
            }
        )
        if label is not UNSET:
            field_dict["label"] = label

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.simulation_create_audience_item import (
            SimulationCreateAudienceItem,
        )

        d = dict(src_dict)
        audience = []
        _audience = d.pop("audience")
        for audience_item_data in _audience:
            audience_item = SimulationCreateAudienceItem.from_dict(audience_item_data)

            audience.append(audience_item)

        label = d.pop("label", UNSET)

        simulation_create = cls(
            audience=audience,
            label=label,
        )

        simulation_create.additional_properties = d
        return simulation_create

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
