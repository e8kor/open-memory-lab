from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="Denial")


@_attrs_define
class Denial:
    """
    Attributes:
        denial_id (str):
        workarea_node_id (str):
        epoch (int):
        lifted (bool | Unset):
    """

    denial_id: str
    workarea_node_id: str
    epoch: int
    lifted: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        denial_id = self.denial_id

        workarea_node_id = self.workarea_node_id

        epoch = self.epoch

        lifted = self.lifted

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "denial_id": denial_id,
                "workarea_node_id": workarea_node_id,
                "epoch": epoch,
            }
        )
        if lifted is not UNSET:
            field_dict["lifted"] = lifted

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        denial_id = d.pop("denial_id")

        workarea_node_id = d.pop("workarea_node_id")

        epoch = d.pop("epoch")

        lifted = d.pop("lifted", UNSET)

        denial = cls(
            denial_id=denial_id,
            workarea_node_id=workarea_node_id,
            epoch=epoch,
            lifted=lifted,
        )

        denial.additional_properties = d
        return denial

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
