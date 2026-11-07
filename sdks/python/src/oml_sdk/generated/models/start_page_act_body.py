from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.start_page_act_body_action import StartPageActBodyAction
from ..types import UNSET, Unset

T = TypeVar("T", bound="StartPageActBody")


@_attrs_define
class StartPageActBody:
    """
    Attributes:
        action (StartPageActBodyAction):
        csrf_token (str):
        confirm (str | Unset):
    """

    action: StartPageActBodyAction
    csrf_token: str
    confirm: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        action = self.action.value

        csrf_token = self.csrf_token

        confirm = self.confirm

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "action": action,
                "csrf_token": csrf_token,
            }
        )
        if confirm is not UNSET:
            field_dict["confirm"] = confirm

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        action = StartPageActBodyAction(d.pop("action"))

        csrf_token = d.pop("csrf_token")

        confirm = d.pop("confirm", UNSET)

        start_page_act_body = cls(
            action=action,
            csrf_token=csrf_token,
            confirm=confirm,
        )

        start_page_act_body.additional_properties = d
        return start_page_act_body

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
