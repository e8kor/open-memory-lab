from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.recall_budget_counter import RecallBudgetCounter

T = TypeVar("T", bound="RecallBudget")


@_attrs_define
class RecallBudget:
    """How the max_tokens cut was applied (docs/architecture/retrieval.md, Token budgets). The example is steering [120]
    and hits [300, 250, 90] under max_tokens 500: one hit is served, and the 90-token hit is not, because the cut is a
    strict rank prefix.

        Example:
            {'max_tokens': 500, 'counter': 'oml-tc/1', 'steering_tokens': 120, 'used_tokens': 420, 'truncated': True,
                'hits_omitted': 2, 'next_hit_tokens': 250}

        Attributes:
            max_tokens (int):
            counter (RecallBudgetCounter):
            steering_tokens (int):
            used_tokens (int): Exceeds max_tokens only when steering alone does.
            truncated (bool):
            hits_omitted (int):
            next_hit_tokens (int | None): Budget tokens of the first hit not served; null when nothing was cut.
    """

    max_tokens: int
    counter: RecallBudgetCounter
    steering_tokens: int
    used_tokens: int
    truncated: bool
    hits_omitted: int
    next_hit_tokens: int | None

    def to_dict(self) -> dict[str, Any]:
        max_tokens = self.max_tokens

        counter = self.counter.value

        steering_tokens = self.steering_tokens

        used_tokens = self.used_tokens

        truncated = self.truncated

        hits_omitted = self.hits_omitted

        next_hit_tokens: int | None
        next_hit_tokens = self.next_hit_tokens

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "max_tokens": max_tokens,
                "counter": counter,
                "steering_tokens": steering_tokens,
                "used_tokens": used_tokens,
                "truncated": truncated,
                "hits_omitted": hits_omitted,
                "next_hit_tokens": next_hit_tokens,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        max_tokens = d.pop("max_tokens")

        counter = RecallBudgetCounter(d.pop("counter"))

        steering_tokens = d.pop("steering_tokens")

        used_tokens = d.pop("used_tokens")

        truncated = d.pop("truncated")

        hits_omitted = d.pop("hits_omitted")

        def _parse_next_hit_tokens(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        next_hit_tokens = _parse_next_hit_tokens(d.pop("next_hit_tokens"))

        recall_budget = cls(
            max_tokens=max_tokens,
            counter=counter,
            steering_tokens=steering_tokens,
            used_tokens=used_tokens,
            truncated=truncated,
            hits_omitted=hits_omitted,
            next_hit_tokens=next_hit_tokens,
        )

        return recall_budget
