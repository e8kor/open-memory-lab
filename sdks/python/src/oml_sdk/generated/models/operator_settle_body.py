from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.operator_settle_body_outcome import OperatorSettleBodyOutcome

T = TypeVar("T", bound="OperatorSettleBody")


@_attrs_define
class OperatorSettleBody:
    """
    Attributes:
        outcome (OperatorSettleBodyOutcome):
    """

    outcome: OperatorSettleBodyOutcome

    def to_dict(self) -> dict[str, Any]:
        outcome = self.outcome.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "outcome": outcome,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        outcome = OperatorSettleBodyOutcome(d.pop("outcome"))

        operator_settle_body = cls(
            outcome=outcome,
        )

        return operator_settle_body
