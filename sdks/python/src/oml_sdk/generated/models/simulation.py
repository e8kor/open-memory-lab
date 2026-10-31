from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.simulation_state import SimulationState
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.simulation_audience_item import SimulationAudienceItem


T = TypeVar("T", bound="Simulation")


@_attrs_define
class Simulation:
    """
    Attributes:
        simulation_id (str):
        state (SimulationState):
        audience (list[SimulationAudienceItem]):
        label (str | Unset):
        opened_at (datetime.datetime | Unset):
        ended_at (datetime.datetime | None | Unset):
        warnings (list[str] | Unset): E.g. no dreamer in the audience — nothing can promote (fail closed).
    """

    simulation_id: str
    state: SimulationState
    audience: list[SimulationAudienceItem]
    label: str | Unset = UNSET
    opened_at: datetime.datetime | Unset = UNSET
    ended_at: datetime.datetime | None | Unset = UNSET
    warnings: list[str] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        simulation_id = self.simulation_id

        state = self.state.value

        audience = []
        for audience_item_data in self.audience:
            audience_item = audience_item_data.to_dict()
            audience.append(audience_item)

        label = self.label

        opened_at: str | Unset = UNSET
        if not isinstance(self.opened_at, Unset):
            opened_at = self.opened_at.isoformat()

        ended_at: None | str | Unset
        if isinstance(self.ended_at, Unset):
            ended_at = UNSET
        elif isinstance(self.ended_at, datetime.datetime):
            ended_at = self.ended_at.isoformat()
        else:
            ended_at = self.ended_at

        warnings: list[str] | Unset = UNSET
        if not isinstance(self.warnings, Unset):
            warnings = self.warnings

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "simulation_id": simulation_id,
                "state": state,
                "audience": audience,
            }
        )
        if label is not UNSET:
            field_dict["label"] = label
        if opened_at is not UNSET:
            field_dict["opened_at"] = opened_at
        if ended_at is not UNSET:
            field_dict["ended_at"] = ended_at
        if warnings is not UNSET:
            field_dict["warnings"] = warnings

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.simulation_audience_item import SimulationAudienceItem

        d = dict(src_dict)
        simulation_id = d.pop("simulation_id")

        state = SimulationState(d.pop("state"))

        audience = []
        _audience = d.pop("audience")
        for audience_item_data in _audience:
            audience_item = SimulationAudienceItem.from_dict(audience_item_data)

            audience.append(audience_item)

        label = d.pop("label", UNSET)

        _opened_at = d.pop("opened_at", UNSET)
        opened_at: datetime.datetime | Unset
        if isinstance(_opened_at, Unset):
            opened_at = UNSET
        else:
            opened_at = datetime.datetime.fromisoformat(_opened_at)

        def _parse_ended_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                ended_at_type_0 = datetime.datetime.fromisoformat(data)

                return ended_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        ended_at = _parse_ended_at(d.pop("ended_at", UNSET))

        warnings = cast(list[str], d.pop("warnings", UNSET))

        simulation = cls(
            simulation_id=simulation_id,
            state=state,
            audience=audience,
            label=label,
            opened_at=opened_at,
            ended_at=ended_at,
            warnings=warnings,
        )

        simulation.additional_properties = d
        return simulation

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
