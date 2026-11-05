from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="UsageSettings")


@_attrs_define
class UsageSettings:
    """
    Attributes:
        recording (bool): Whether this server records recalls.
        query_text (bool): Whether the tenant's recalls keep their query text (telemetry.query_text).
        query_text_since (None | str): When it was turned on; null when off.
        retention_days (int): How long an event and its labels are kept: 30.
    """

    recording: bool
    query_text: bool
    query_text_since: None | str
    retention_days: int

    def to_dict(self) -> dict[str, Any]:
        recording = self.recording

        query_text = self.query_text

        query_text_since: None | str
        query_text_since = self.query_text_since

        retention_days = self.retention_days

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "recording": recording,
                "query_text": query_text,
                "query_text_since": query_text_since,
                "retention_days": retention_days,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        recording = d.pop("recording")

        query_text = d.pop("query_text")

        def _parse_query_text_since(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        query_text_since = _parse_query_text_since(d.pop("query_text_since"))

        retention_days = d.pop("retention_days")

        usage_settings = cls(
            recording=recording,
            query_text=query_text,
            query_text_since=query_text_since,
            retention_days=retention_days,
        )

        return usage_settings
