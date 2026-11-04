from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="DenialRequest")


@_attrs_define
class DenialRequest:
    """
    Attributes:
        workarea (str): A workarea, by its node id or its name. Resolved server-side before any check that reads state;
            an unknown name answers exactly what an unknown id answers. A derived path is neither, and is refused
            (request.validation).
        reason (str | Unset):
    """

    workarea: str
    reason: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        workarea: str
        workarea = self.workarea

        reason = self.reason

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "workarea": workarea,
            }
        )
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_workarea(data: object) -> str:
            return cast(str, data)

        workarea = _parse_workarea(d.pop("workarea"))

        reason = d.pop("reason", UNSET)

        denial_request = cls(
            workarea=workarea,
            reason=reason,
        )

        return denial_request
