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
            lifetime past ten minutes is minted only for a connector key whose grants all sit on one workarea other than the
            universe and that names no user root; otherwise 422 oauth.connect_refused with details.reason not_confined.
            Outside 1 to 2592000 is request.validation naming /ttl_seconds (ADR-0067 d3).
    """

    ttl_seconds: int | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        ttl_seconds = self.ttl_seconds

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if ttl_seconds is not UNSET:
            field_dict["ttl_seconds"] = ttl_seconds

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        ttl_seconds = d.pop("ttl_seconds", UNSET)

        mint_connect_code_body = cls(
            ttl_seconds=ttl_seconds,
        )

        return mint_connect_code_body
