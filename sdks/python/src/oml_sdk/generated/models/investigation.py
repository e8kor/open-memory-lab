from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.investigation_state import InvestigationState

T = TypeVar("T", bound="Investigation")


@_attrs_define
class Investigation:
    """An admin key's investigation, the reason its reads of claim text are made under (ADR-0089 d10, d15). Since v0.8.4.

    Attributes:
        investigation_id (UUID):
        reason (str):
        opened_at (datetime.datetime):
        reads (int): The admin reads made under it.
        age_seconds (int):
        state (InvestigationState):
        closed_at (datetime.datetime | None):
    """

    investigation_id: UUID
    reason: str
    opened_at: datetime.datetime
    reads: int
    age_seconds: int
    state: InvestigationState
    closed_at: datetime.datetime | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        investigation_id = str(self.investigation_id)

        reason = self.reason

        opened_at = self.opened_at.isoformat()

        reads = self.reads

        age_seconds = self.age_seconds

        state = self.state.value

        closed_at: None | str
        if isinstance(self.closed_at, datetime.datetime):
            closed_at = self.closed_at.isoformat()
        else:
            closed_at = self.closed_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "investigation_id": investigation_id,
                "reason": reason,
                "opened_at": opened_at,
                "reads": reads,
                "age_seconds": age_seconds,
                "state": state,
                "closed_at": closed_at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        investigation_id = UUID(d.pop("investigation_id"))

        reason = d.pop("reason")

        opened_at = datetime.datetime.fromisoformat(d.pop("opened_at"))

        reads = d.pop("reads")

        age_seconds = d.pop("age_seconds")

        state = InvestigationState(d.pop("state"))

        def _parse_closed_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                closed_at_type_0 = datetime.datetime.fromisoformat(data)

                return closed_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        closed_at = _parse_closed_at(d.pop("closed_at"))

        investigation = cls(
            investigation_id=investigation_id,
            reason=reason,
            opened_at=opened_at,
            reads=reads,
            age_seconds=age_seconds,
            state=state,
            closed_at=closed_at,
        )

        investigation.additional_properties = d
        return investigation

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
