from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.workarea_mode_set_mode import WorkareaModeSetMode

T = TypeVar("T", bound="WorkareaModeSet")


@_attrs_define
class WorkareaModeSet:
    """
    Attributes:
        mode (WorkareaModeSetMode): in_progress puts the node in progress; standard on an in-progress root is its commit
            (ADR-0030 d1, d6).
    """

    mode: WorkareaModeSetMode

    def to_dict(self) -> dict[str, Any]:
        mode = self.mode.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "mode": mode,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        mode = WorkareaModeSetMode(d.pop("mode"))

        workarea_mode_set = cls(
            mode=mode,
        )

        return workarea_mode_set
