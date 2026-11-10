from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="RequestWebDeletionResponse200")


@_attrs_define
class RequestWebDeletionResponse200:
    """
    Attributes:
        requested_at (datetime.datetime | None):
        journal_seq (int): The seq of web.deletion_requested.
    """

    requested_at: datetime.datetime | None
    journal_seq: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        requested_at: None | str
        if isinstance(self.requested_at, datetime.datetime):
            requested_at = self.requested_at.isoformat()
        else:
            requested_at = self.requested_at

        journal_seq = self.journal_seq

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "requested_at": requested_at,
                "journal_seq": journal_seq,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_requested_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                requested_at_type_0 = datetime.datetime.fromisoformat(data)

                return requested_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        requested_at = _parse_requested_at(d.pop("requested_at"))

        journal_seq = d.pop("journal_seq")

        request_web_deletion_response_200 = cls(
            requested_at=requested_at,
            journal_seq=journal_seq,
        )

        request_web_deletion_response_200.additional_properties = d
        return request_web_deletion_response_200

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
