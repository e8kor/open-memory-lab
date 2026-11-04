from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="RevokeConnectionBody")


@_attrs_define
class RevokeConnectionBody:
    """
    Attributes:
        reason (str | Unset): Why; the journal keeps its SHA-256 digest, never the text.
    """

    reason: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        reason = self.reason

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        reason = d.pop("reason", UNSET)

        revoke_connection_body = cls(
            reason=reason,
        )

        return revoke_connection_body
