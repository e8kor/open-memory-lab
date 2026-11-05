from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="SetUsageSettingsBody")


@_attrs_define
class SetUsageSettingsBody:
    """
    Attributes:
        query_text (bool):
    """

    query_text: bool

    def to_dict(self) -> dict[str, Any]:
        query_text = self.query_text

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "query_text": query_text,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        query_text = d.pop("query_text")

        set_usage_settings_body = cls(
            query_text=query_text,
        )

        return set_usage_settings_body
