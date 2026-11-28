from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="AdminTodoCountsResponse200")


@_attrs_define
class AdminTodoCountsResponse200:
    """
    Attributes:
        alerts (int): The guardrail-loosening alerts: unconfirmed fixes that loosen a guardrail (ADR-0096 d7). Since
            v0.8.5.
        acts_to_confirm (int): Admin acts to confirm (ADR-0097 d2, d10, d11): the tenant's widening acts the calling
            admin person may settle, never their own: pending acts unexpired, taken at least seven days after the person's
            admin flag was accepted, and acts taken at once under sole_admin once that flag is seven days old. Since v0.8.6.
        truncated (bool):
    """

    alerts: int
    acts_to_confirm: int
    truncated: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        alerts = self.alerts

        acts_to_confirm = self.acts_to_confirm

        truncated = self.truncated

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "alerts": alerts,
                "acts_to_confirm": acts_to_confirm,
                "truncated": truncated,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        alerts = d.pop("alerts")

        acts_to_confirm = d.pop("acts_to_confirm")

        truncated = d.pop("truncated")

        admin_todo_counts_response_200 = cls(
            alerts=alerts,
            acts_to_confirm=acts_to_confirm,
            truncated=truncated,
        )

        admin_todo_counts_response_200.additional_properties = d
        return admin_todo_counts_response_200

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
