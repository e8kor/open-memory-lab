from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="WorkspaceViewApprovedByKind")


@_attrs_define
class WorkspaceViewApprovedByKind:
    """The approved claims on this node alone, not its subtree, by kind: a claim counts once when its tip is promoted and
    is not a retirement, so a replaced version or a held successor adds nothing. Only kinds with at least one approved
    claim are named (ADR-0082 d8 item 7, d10). Since v0.8.3.

    """

    additional_properties: dict[str, int] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        workspace_view_approved_by_kind = cls()

        workspace_view_approved_by_kind.additional_properties = d
        return workspace_view_approved_by_kind

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> int:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: int) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
