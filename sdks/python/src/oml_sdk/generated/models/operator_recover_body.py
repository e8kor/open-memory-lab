from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="OperatorRecoverBody")


@_attrs_define
class OperatorRecoverBody:
    """
    Attributes:
        person (str): The admin person the recovery adds: a name the tenant holds as no workspace, key or entry.
        email (str): The address the new admin person signs in with; no entry may hold it.
    """

    person: str
    email: str

    def to_dict(self) -> dict[str, Any]:
        person = self.person

        email = self.email

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "person": person,
                "email": email,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        person = d.pop("person")

        email = d.pop("email")

        operator_recover_body = cls(
            person=person,
            email=email,
        )

        return operator_recover_body
