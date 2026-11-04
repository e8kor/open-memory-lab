from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="WebEmail")


@_attrs_define
class WebEmail:
    """An allowlist entry (ADR-0045 d2): an email a person may sign in with, the ADR-0044 user it signs in as, and whether
    it opens the admin page.

        Attributes:
            email (str):
            user (str): A name under the creation grammar, at most 56 characters.
            admin (bool):
    """

    email: str
    user: str
    admin: bool

    def to_dict(self) -> dict[str, Any]:
        email = self.email

        user = self.user

        admin = self.admin

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "email": email,
                "user": user,
                "admin": admin,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        email = d.pop("email")

        user = d.pop("user")

        admin = d.pop("admin")

        web_email = cls(
            email=email,
            user=user,
            admin=admin,
        )

        return web_email
