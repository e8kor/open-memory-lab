from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.close_web_deletion_body_outcome import CloseWebDeletionBodyOutcome

T = TypeVar("T", bound="CloseWebDeletionBody")


@_attrs_define
class CloseWebDeletionBody:
    """
    Attributes:
        outcome (CloseWebDeletionBodyOutcome):
    """

    outcome: CloseWebDeletionBodyOutcome

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
        outcome = CloseWebDeletionBodyOutcome(d.pop("outcome"))

        close_web_deletion_body = cls(
            outcome=outcome,
        )

        return close_web_deletion_body
