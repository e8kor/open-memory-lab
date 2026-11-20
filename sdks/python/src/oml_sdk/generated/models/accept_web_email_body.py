from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="AcceptWebEmailBody")


@_attrs_define
class AcceptWebEmailBody:
    """
    Attributes:
        email (str):
        admin (bool): The admin flag of the entry as the person reviewed it, from whoami's pending_emails; an entry
            whose flag is not this one, an admin having changed it since, is refused request.validation, details.field
            /admin, details.reason changed, and journals nothing (the operator's decision of 2026-11-20). Since v0.8.4.
    """

    email: str
    admin: bool

    def to_dict(self) -> dict[str, Any]:
        email = self.email

        admin = self.admin

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "email": email,
                "admin": admin,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        email = d.pop("email")

        admin = d.pop("admin")

        accept_web_email_body = cls(
            email=email,
            admin=admin,
        )

        return accept_web_email_body
