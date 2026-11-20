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
            pending (bool): Whether the entry waits on its person's acceptance: an admin added it, or changed it to name
                them, for a person who signs in already; it signs in to nothing, auth.invalid with details.reason email_pending,
                until that person accepts it with POST /v2/web/emails/accept (ADR-0045 d2, the operator's decision of
                2026-11-19). Since v0.8.4.
    """

    email: str
    user: str
    admin: bool
    pending: bool

    def to_dict(self) -> dict[str, Any]:
        email = self.email

        user = self.user

        admin = self.admin

        pending = self.pending

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "email": email,
                "user": user,
                "admin": admin,
                "pending": pending,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        email = d.pop("email")

        user = d.pop("user")

        admin = d.pop("admin")

        pending = d.pop("pending")

        web_email = cls(
            email=email,
            user=user,
            admin=admin,
            pending=pending,
        )

        return web_email
