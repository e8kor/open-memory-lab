from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.served_guardrail_verdicts_item_held_by import (
    ServedGuardrailVerdictsItemHeldBy,
)

T = TypeVar("T", bound="ServedGuardrailVerdictsItem")


@_attrs_define
class ServedGuardrailVerdictsItem:
    """
    Attributes:
        held_by (ServedGuardrailVerdictsItemHeldBy):
        at (datetime.datetime):
    """

    held_by: ServedGuardrailVerdictsItemHeldBy
    at: datetime.datetime

    def to_dict(self) -> dict[str, Any]:
        held_by = self.held_by.value

        at = self.at.isoformat()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "held_by": held_by,
                "at": at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        held_by = ServedGuardrailVerdictsItemHeldBy(d.pop("held_by"))

        at = datetime.datetime.fromisoformat(d.pop("at"))

        served_guardrail_verdicts_item = cls(
            held_by=held_by,
            at=at,
        )

        return served_guardrail_verdicts_item
