from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.web_sign_in_body_provider import WebSignInBodyProvider
from ..types import UNSET, Unset

T = TypeVar("T", bound="WebSignInBody")


@_attrs_define
class WebSignInBody:
    """
    Attributes:
        provider (WebSignInBodyProvider):
        next_ (str | Unset): The page to return to after the callback, admitted as GET /web/sign-in admits it and sealed
            with the sign-in state (ADR-0070 d4).
    """

    provider: WebSignInBodyProvider
    next_: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        provider = self.provider.value

        next_ = self.next_

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "provider": provider,
            }
        )
        if next_ is not UNSET:
            field_dict["next"] = next_

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        provider = WebSignInBodyProvider(d.pop("provider"))

        next_ = d.pop("next", UNSET)

        web_sign_in_body = cls(
            provider=provider,
            next_=next_,
        )

        return web_sign_in_body
