from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.web_sign_in_body_provider import WebSignInBodyProvider

T = TypeVar("T", bound="WebSignInBody")


@_attrs_define
class WebSignInBody:
    """
    Attributes:
        provider (WebSignInBodyProvider):
    """

    provider: WebSignInBodyProvider

    def to_dict(self) -> dict[str, Any]:
        provider = self.provider.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "provider": provider,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        provider = WebSignInBodyProvider(d.pop("provider"))

        web_sign_in_body = cls(
            provider=provider,
        )

        return web_sign_in_body
