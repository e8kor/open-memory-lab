from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="AuthorizeSubmitBodyType0")


@_attrs_define
class AuthorizeSubmitBodyType0:
    """Step 2: the connect code.

    Attributes:
        handle (str): The authorization request's handle the page carries.
        connect_code (str): The code, normalized by Crockford base32 decoding.
    """

    handle: str
    connect_code: str

    def to_dict(self) -> dict[str, Any]:
        handle = self.handle

        connect_code = self.connect_code

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "handle": handle,
                "connect_code": connect_code,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        handle = d.pop("handle")

        connect_code = d.pop("connect_code")

        authorize_submit_body_type_0 = cls(
            handle=handle,
            connect_code=connect_code,
        )

        return authorize_submit_body_type_0
