from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="GuardrailSetOwnHeldItem")


@_attrs_define
class GuardrailSetOwnHeldItem:
    """
    Attributes:
        claim_key (str):
        version (int):
    """

    claim_key: str
    version: int

    def to_dict(self) -> dict[str, Any]:
        claim_key = self.claim_key

        version = self.version

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "claim_key": claim_key,
                "version": version,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        claim_key = d.pop("claim_key")

        version = d.pop("version")

        guardrail_set_own_held_item = cls(
            claim_key=claim_key,
            version=version,
        )

        return guardrail_set_own_held_item
