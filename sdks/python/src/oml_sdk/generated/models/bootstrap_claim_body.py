from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

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
    """

    enrollment_token: str
    name: str

    def to_dict(self) -> dict[str, Any]:
        enrollment_token = self.enrollment_token

        name = self.name

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "enrollment_token": enrollment_token,
                "name": name,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        enrollment_token = d.pop("enrollment_token")

        name = d.pop("name")

        bootstrap_claim_body = cls(
            enrollment_token=enrollment_token,
            name=name,
        )

        return bootstrap_claim_body
