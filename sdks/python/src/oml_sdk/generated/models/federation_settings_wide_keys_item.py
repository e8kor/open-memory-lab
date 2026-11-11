from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define

T = TypeVar("T", bound="FederationSettingsWideKeysItem")


@_attrs_define
class FederationSettingsWideKeysItem:
    """
    Attributes:
        key_id (UUID):
        candidates (int):
    """

    key_id: UUID
    candidates: int

    def to_dict(self) -> dict[str, Any]:
        key_id = str(self.key_id)

        candidates = self.candidates

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "key_id": key_id,
                "candidates": candidates,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        key_id = UUID(d.pop("key_id"))

        candidates = d.pop("candidates")

        federation_settings_wide_keys_item = cls(
            key_id=key_id,
            candidates=candidates,
        )

        return federation_settings_wide_keys_item
