from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define

from ..models.operator_recover_response_202_recovery_state import (
    OperatorRecoverResponse202RecoveryState,
)

T = TypeVar("T", bound="OperatorRecoverResponse202Recovery")


@_attrs_define
class OperatorRecoverResponse202Recovery:
    """
    Attributes:
        recovery_id (UUID):
        person (str):
        state (OperatorRecoverResponse202RecoveryState):
        requested_at (datetime.datetime):
        completes_after (datetime.datetime | None):
        settled_at (datetime.datetime | None):
    """

    recovery_id: UUID
    person: str
    state: OperatorRecoverResponse202RecoveryState
    requested_at: datetime.datetime
    completes_after: datetime.datetime | None
    settled_at: datetime.datetime | None

    def to_dict(self) -> dict[str, Any]:
        recovery_id = str(self.recovery_id)

        person = self.person

        state = self.state.value

        requested_at = self.requested_at.isoformat()

        completes_after: None | str
        if isinstance(self.completes_after, datetime.datetime):
            completes_after = self.completes_after.isoformat()
        else:
            completes_after = self.completes_after

        settled_at: None | str
        if isinstance(self.settled_at, datetime.datetime):
            settled_at = self.settled_at.isoformat()
        else:
            settled_at = self.settled_at

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "recovery_id": recovery_id,
                "person": person,
                "state": state,
                "requested_at": requested_at,
                "completes_after": completes_after,
                "settled_at": settled_at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        recovery_id = UUID(d.pop("recovery_id"))

        person = d.pop("person")

        state = OperatorRecoverResponse202RecoveryState(d.pop("state"))

        requested_at = datetime.datetime.fromisoformat(d.pop("requested_at"))

        def _parse_completes_after(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                completes_after_type_0 = datetime.datetime.fromisoformat(data)

                return completes_after_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        completes_after = _parse_completes_after(d.pop("completes_after"))

        def _parse_settled_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                settled_at_type_0 = datetime.datetime.fromisoformat(data)

                return settled_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        settled_at = _parse_settled_at(d.pop("settled_at"))

        operator_recover_response_202_recovery = cls(
            recovery_id=recovery_id,
            person=person,
            state=state,
            requested_at=requested_at,
            completes_after=completes_after,
            settled_at=settled_at,
        )

        return operator_recover_response_202_recovery
