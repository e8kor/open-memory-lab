from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="DeclineWebEmailBody")


@_attrs_define
class DeclineWebEmailBody:
    """
    Attributes:
        email (str):
    """

    email: str

    def to_dict(self) -> dict[str, Any]:
        email = self.email

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "email": email,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        email = d.pop("email")

        decline_web_email_body = cls(
            email=email,
        )

        return decline_web_email_body
