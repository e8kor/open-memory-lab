from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="BootstrapClaimBody")


@_attrs_define
class BootstrapClaimBody:
    """
    Attributes:
        enrollment_token (str):
        name (str): A new name, in a creation body: the creation grammar. A lowercase RFC 1035 label, 1-63 characters,
            that is not in the id form, not 32 hexadecimal digits and not the derived form, which only rows that predate
            names hold; a creation that sends that form is request.validation. Unique in its namespace, immutable, never
            released; compared byte for byte, never lowercased (docs/api/http-api.md, Names and ids; ADR-0026 d1, d2).
            References use NameRef.
        person (str | Unset): A new name, in a creation body: the creation grammar. A lowercase RFC 1035 label, 1-63
            characters, that is not in the id form, not 32 hexadecimal digits and not the derived form, which only rows that
            predate names hold; a creation that sends that form is request.validation. Unique in its namespace, immutable,
            never released; compared byte for byte, never lowercased (docs/api/http-api.md, Names and ids; ADR-0026 d1, d2).
            References use NameRef.
        email (str | Unset): The sign-in email of the person person names, recorded as that person's accepted admin
            entry, journalled as web.email_allowed with pending false: the one way the person a claim names gets a first
            sign-in, since an email an admin adds later for a person a key names waits on that person (ADR-0045 d2's note,
            the operator's decision of 2026-11-20). An email another tenant's entry holds is web.email_taken. Since v0.8.4.
    """

    enrollment_token: str
    name: str
    person: str | Unset = UNSET
    email: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        enrollment_token = self.enrollment_token

        name = self.name

        person = self.person

        email = self.email

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "enrollment_token": enrollment_token,
                "name": name,
            }
        )
        if person is not UNSET:
            field_dict["person"] = person
        if email is not UNSET:
            field_dict["email"] = email

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        enrollment_token = d.pop("enrollment_token")

        name = d.pop("name")

        person = d.pop("person", UNSET)

        email = d.pop("email", UNSET)

        bootstrap_claim_body = cls(
            enrollment_token=enrollment_token,
            name=name,
            person=person,
            email=email,
        )

        return bootstrap_claim_body
