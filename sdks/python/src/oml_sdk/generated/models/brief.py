from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.memory import Memory
    from ..models.recall_budget import RecallBudget


T = TypeVar("T", bound="Brief")


@_attrs_define
class Brief:
    """The workarea's ratified brief as this key reads it, in ratification order, cut by brief.max_tokens; empty for an
    admin key (ADR-0046 d8, d9).

        Attributes:
            items (list[Memory]):
            budget (RecallBudget): How the max_tokens cut was applied (docs/architecture/retrieval.md, Token budgets). The
                example is steering [120] and hits [300, 250, 90] under max_tokens 500: one hit is served, and the 90-token hit
                is not, because the cut is a strict rank prefix. Example: {'max_tokens': 500, 'counter': 'oml-tc/1',
                'steering_tokens': 120, 'used_tokens': 420, 'truncated': True, 'hits_omitted': 2, 'next_hit_tokens': 250}.
    """

    items: list[Memory]
    budget: RecallBudget

    def to_dict(self) -> dict[str, Any]:
        items = []
        for items_item_data in self.items:
            items_item = items_item_data.to_dict()
            items.append(items_item)

        budget = self.budget.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "items": items,
                "budget": budget,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.memory import Memory
        from ..models.recall_budget import RecallBudget

        d = dict(src_dict)
        items = []
        _items = d.pop("items")
        for items_item_data in _items:
            items_item = Memory.from_dict(items_item_data)

            items.append(items_item)

        budget = RecallBudget.from_dict(d.pop("budget"))

        brief = cls(
            items=items,
            budget=budget,
        )

        return brief
