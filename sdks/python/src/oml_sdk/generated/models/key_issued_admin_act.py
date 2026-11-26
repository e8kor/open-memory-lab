from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.key_issued_admin_act_state import KeyIssuedAdminActState

T = TypeVar("T", bound="KeyIssuedAdminAct")


@_attrs_define
class KeyIssuedAdminAct:
    """An admin key's issuance is a widening admin act (ADR-0097 d5). state pending_confirmation: the value or code is
    shown once here, and the key authenticates nothing (auth.invalid, details.reason pending_confirmation) and holds no
    grant until a second admin person confirms the act with POST /v2/admin/acts/{act}/confirm; a connect code's ten
    minutes run from the confirmation. state effective: the tenant has never had two live admin persons, and the key
    took effect at once, marked sole_admin (ADR-0097 d10). Absent for a person's own issuance. Since v0.8.6.

        Attributes:
            act_id (UUID):
            state (KeyIssuedAdminActState):
    """

    act_id: UUID
    state: KeyIssuedAdminActState
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

        state = KeyIssuedAdminActState(d.pop("state"))

        key_issued_admin_act = cls(
            act_id=act_id,
            state=state,
        )

        key_issued_admin_act.additional_properties = d
        return key_issued_admin_act

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
