from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.served_guardrail_verdicts_item_used_by import (
    ServedGuardrailVerdictsItemUsedBy,
)

T = TypeVar("T", bound="ServedGuardrailVerdictsItem")


@_attrs_define
class ServedGuardrailVerdictsItem:
    """
    Attributes:
        used_by (ServedGuardrailVerdictsItemUsedBy):
        at (datetime.datetime):
    """

    used_by: ServedGuardrailVerdictsItemUsedBy
    at: datetime.datetime

    def to_dict(self) -> dict[str, Any]:
        used_by = self.used_by.value

        at = self.at.isoformat()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "used_by": used_by,
                "at": at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        used_by = ServedGuardrailVerdictsItemUsedBy(d.pop("used_by"))

        at = datetime.datetime.fromisoformat(d.pop("at"))

        served_guardrail_verdicts_item = cls(
            used_by=used_by,
            at=at,
        )

        return served_guardrail_verdicts_item
