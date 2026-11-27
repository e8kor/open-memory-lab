from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="LinkSelfBody")


@_attrs_define
class LinkSelfBody:
    """
    Attributes:
        user (str):
        reason (str):
    """

    user: str
    reason: str

    def to_dict(self) -> dict[str, Any]:
        user = self.user

        reason = self.reason

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "user": user,
                "reason": reason,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        user = d.pop("user")

        reason = d.pop("reason")

        link_self_body = cls(
            user=user,
            reason=reason,
        )

        return link_self_body
