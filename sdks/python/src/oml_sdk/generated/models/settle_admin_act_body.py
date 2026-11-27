from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.settle_admin_act_body_outcome import SettleAdminActBodyOutcome

T = TypeVar("T", bound="SettleAdminActBody")


@_attrs_define
class SettleAdminActBody:
    """
    Attributes:
        outcome (SettleAdminActBodyOutcome):
    """

    outcome: SettleAdminActBodyOutcome

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
        outcome = SettleAdminActBodyOutcome(d.pop("outcome"))

        settle_admin_act_body = cls(
            outcome=outcome,
        )

        return settle_admin_act_body
