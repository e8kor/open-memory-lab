from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.workarea_mode_set_children import WorkareaModeSetChildren
from ..models.workarea_mode_set_mode import WorkareaModeSetMode
from ..types import UNSET, Unset

T = TypeVar("T", bound="WorkareaModeSet")


@_attrs_define
class WorkareaModeSet:
    """
    Attributes:
        mode (WorkareaModeSetMode | Unset): in_progress puts the node in progress; standard on an in-progress root is
            its commit (ADR-0030 d1, d6).
        children (WorkareaModeSetChildren | Unset): agent marks the node a code parent: agent writer keys then create
            in-progress children under it and nowhere else; none unmarks it, and children already created keep their mode.
            Send exactly one of mode and children (ADR-0064 d3).
    """

    mode: WorkareaModeSetMode | Unset = UNSET
    children: WorkareaModeSetChildren | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        mode: str | Unset = UNSET
        if not isinstance(self.mode, Unset):
            mode = self.mode.value

        children: str | Unset = UNSET
        if not isinstance(self.children, Unset):
            children = self.children.value

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if mode is not UNSET:
            field_dict["mode"] = mode
        if children is not UNSET:
            field_dict["children"] = children

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        _mode = d.pop("mode", UNSET)
        mode: WorkareaModeSetMode | Unset
        if isinstance(_mode, Unset):
            mode = UNSET
        else:
            mode = WorkareaModeSetMode(_mode)

        _children = d.pop("children", UNSET)
        children: WorkareaModeSetChildren | Unset
        if isinstance(_children, Unset):
            children = UNSET
        else:
            children = WorkareaModeSetChildren(_children)

        workarea_mode_set = cls(
            mode=mode,
            children=children,
        )

        return workarea_mode_set
