from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="AdminOpenInvestigationBody")


@_attrs_define
class AdminOpenInvestigationBody:
    """
    Attributes:
        reason (str):
    """

    reason: str

    def to_dict(self) -> dict[str, Any]:
        reason = self.reason

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "reason": reason,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        reason = d.pop("reason")

        admin_open_investigation_body = cls(
            reason=reason,
        )

        return admin_open_investigation_body
