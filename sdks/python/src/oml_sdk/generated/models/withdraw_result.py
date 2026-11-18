from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.withdraw_result_state import WithdrawResultState

T = TypeVar("T", bound="WithdrawResult")


@_attrs_define
class WithdrawResult:
    """
    Attributes:
        artifact_id (str):
        state (WithdrawResultState):
        at (datetime.datetime):
        journal_seq (int):
    """

    artifact_id: str
    state: WithdrawResultState
    at: datetime.datetime
    journal_seq: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        artifact_id = self.artifact_id

        state = self.state.value

        at = self.at.isoformat()

        journal_seq = self.journal_seq

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "artifact_id": artifact_id,
                "state": state,
                "at": at,
                "journal_seq": journal_seq,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        artifact_id = d.pop("artifact_id")

        state = WithdrawResultState(d.pop("state"))

        at = datetime.datetime.fromisoformat(d.pop("at"))

        journal_seq = d.pop("journal_seq")

        withdraw_result = cls(
            artifact_id=artifact_id,
            state=state,
            at=at,
            journal_seq=journal_seq,
        )

        withdraw_result.additional_properties = d
        return withdraw_result

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
