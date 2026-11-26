from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="MintConnectCodeBody")


@_attrs_define
class MintConnectCodeBody:
    """
    Attributes:
        ttl_seconds (int | Unset): How long the code lives, 1 second to 30 days; omitted, the ten-minute default. A
            lifetime past ten minutes is minted only for a connector key whose grants all sit on one workspace other than
            the universe and that names no user root; otherwise 422 oauth.connect_refused with details.reason not_confined.
            Outside 1 to 2592000 is request.validation naming /ttl_seconds (ADR-0067 d3).
        reason (str | Unset): The admin's reason, scanned and journalled in the mint's admin.act. An admin's mint is
            widening (ADR-0097 d5, d6): outside sole_admin it waits on a second admin person and requires a reason; the code
            is shown once here, voids no earlier code and redeems nothing until confirmed, and its life runs from the
            confirmation. A connector key that itself waits on its issuance's confirmation takes no code: request.validation
            with details.field key and details.reason key_pending. Since v0.8.6.
    """

    ttl_seconds: int | Unset = UNSET
    reason: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        ttl_seconds = self.ttl_seconds

        reason = self.reason

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if ttl_seconds is not UNSET:
            field_dict["ttl_seconds"] = ttl_seconds
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        ttl_seconds = d.pop("ttl_seconds", UNSET)

        reason = d.pop("reason", UNSET)

        mint_connect_code_body = cls(
            ttl_seconds=ttl_seconds,
            reason=reason,
        )

        return mint_connect_code_body
