from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="KeyLinkRequest")


@_attrs_define
class KeyLinkRequest:
    """
    Attributes:
        user (str): The user the key acts for in the console, by name (ADR-0085 d16).
        reason (str): Why the key is linked; required, scanned and journalled with the link's consent act (ADR-0097 d7,
            d26).
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

        key_link_request = cls(
            user=user,
            reason=reason,
        )

        return key_link_request
