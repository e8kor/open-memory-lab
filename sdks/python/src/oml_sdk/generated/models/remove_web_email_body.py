from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="RemoveWebEmailBody")


@_attrs_define
class RemoveWebEmailBody:
    """
    Attributes:
        email (str):
        keep_an_admin (bool | Unset): true: removing the tenant's last admin entry is refused, request.validation with
            details.field admin, and nothing changes (ADR-0045 d10; ADR-0070 d5). Since v0.6.4.
    """

    email: str
    keep_an_admin: bool | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        email = self.email

        keep_an_admin = self.keep_an_admin

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "email": email,
            }
        )
        if keep_an_admin is not UNSET:
            field_dict["keep_an_admin"] = keep_an_admin

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        email = d.pop("email")

        keep_an_admin = d.pop("keep_an_admin", UNSET)

        remove_web_email_body = cls(
            email=email,
            keep_an_admin=keep_an_admin,
        )

        return remove_web_email_body
