from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="RemoveWebEmailResponse200")


@_attrs_define
class RemoveWebEmailResponse200:
    """
    Attributes:
        journal_seq (int): The seq of the web.email_removed event.
        event_id (UUID):
        dry_run (bool):
    """

    journal_seq: int
    event_id: UUID
    dry_run: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        journal_seq = self.journal_seq

        event_id = str(self.event_id)

        dry_run = self.dry_run

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "journal_seq": journal_seq,
                "event_id": event_id,
                "dry_run": dry_run,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        journal_seq = d.pop("journal_seq")

        event_id = UUID(d.pop("event_id"))

        dry_run = d.pop("dry_run")

        remove_web_email_response_200 = cls(
            journal_seq=journal_seq,
            event_id=event_id,
            dry_run=dry_run,
        )

        remove_web_email_response_200.additional_properties = d
        return remove_web_email_response_200

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
