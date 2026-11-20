from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="ServedGuardrailAdminFixType0")


@_attrs_define
class ServedGuardrailAdminFixType0:
    """The mark of a guardrail an admin fix serves, whether a person confirmed or reverted the fix, never which admin; null
    for one a verdict promoted (ADR-0089 d7). Since v0.8.4.

        Attributes:
            confirmed (bool):
            reverted (bool):
    """

    confirmed: bool
    reverted: bool

    def to_dict(self) -> dict[str, Any]:
        confirmed = self.confirmed

        reverted = self.reverted

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "confirmed": confirmed,
                "reverted": reverted,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        confirmed = d.pop("confirmed")

        reverted = d.pop("reverted")

        served_guardrail_admin_fix_type_0 = cls(
            confirmed=confirmed,
            reverted=reverted,
        )

        return served_guardrail_admin_fix_type_0
