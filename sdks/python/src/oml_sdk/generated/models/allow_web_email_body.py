from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="AllowWebEmailBody")


@_attrs_define
class AllowWebEmailBody:
    """
    Attributes:
        email (str):
        user (str): A name under the creation grammar, at most 56 characters.
        admin (bool):
        keep_an_admin (bool | Unset): true: un-flagging the tenant's last admin entry is refused, request.validation
            with details.field admin, and nothing changes; the store reads the admin entries under lock in the commit's own
            transaction (ADR-0045 d10; ADR-0070 d5). Since v0.6.4.
    """

    email: str
    user: str
    admin: bool
    keep_an_admin: bool | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        email = self.email

        user = self.user

        admin = self.admin

        keep_an_admin = self.keep_an_admin

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "email": email,
                "user": user,
                "admin": admin,
            }
        )
        if keep_an_admin is not UNSET:
            field_dict["keep_an_admin"] = keep_an_admin

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        email = d.pop("email")

        user = d.pop("user")

        admin = d.pop("admin")

        keep_an_admin = d.pop("keep_an_admin", UNSET)

        allow_web_email_body = cls(
            email=email,
            user=user,
            admin=admin,
            keep_an_admin=keep_an_admin,
        )

        return allow_web_email_body
