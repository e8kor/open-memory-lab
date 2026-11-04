from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="GrantRequestDenyItem")


@_attrs_define
class GrantRequestDenyItem:
    """
    Attributes:
        workarea (str): A workarea, by its node id or its name. Resolved server-side before any check that reads state;
            an unknown name answers exactly what an unknown id answers. A derived path is neither, and is refused
            (request.validation).
    """

    workarea: str

    def to_dict(self) -> dict[str, Any]:
        workarea: str
        workarea = self.workarea

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "workarea": workarea,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_workarea(data: object) -> str:
            return cast(str, data)

        workarea = _parse_workarea(d.pop("workarea"))

        grant_request_deny_item = cls(
            workarea=workarea,
        )

        return grant_request_deny_item
