from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.key_limits_admin_act_state import KeyLimitsAdminActState

T = TypeVar("T", bound="KeyLimitsAdminAct")


@_attrs_define
class KeyLimitsAdminAct:
    """The change's admin act: pending_confirmation for a raise that waits on a second admin person, effective otherwise
    (ADR-0097 d15). Since v0.8.6.

        Attributes:
            act_id (UUID):
            state (KeyLimitsAdminActState):
    """

    act_id: UUID
    state: KeyLimitsAdminActState
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        act_id = str(self.act_id)

        state = self.state.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "act_id": act_id,
                "state": state,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        act_id = UUID(d.pop("act_id"))

        state = KeyLimitsAdminActState(d.pop("state"))

        key_limits_admin_act = cls(
            act_id=act_id,
            state=state,
        )

        key_limits_admin_act.additional_properties = d
        return key_limits_admin_act

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
