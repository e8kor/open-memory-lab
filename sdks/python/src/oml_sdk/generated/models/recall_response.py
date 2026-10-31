from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.memory import Memory
    from ..models.recall_budget import RecallBudget
    from ..models.recall_response_hits_item import RecallResponseHitsItem


T = TypeVar("T", bound="RecallResponse")


@_attrs_define
class RecallResponse:
    """
    Attributes:
        steering (list[Memory]): Applicable steering, ordered structurally and never cut by a budget — membership and
            order declared in docs/architecture/retrieval.md (ADR-0025 d8).
        hits (list[RecallResponseHitsItem]):
        budget (RecallBudget | Unset): How the max_tokens cut was applied (docs/architecture/retrieval.md, Token
            budgets). The example is steering [120] and hits [300, 250, 90] under max_tokens 500: one hit is served, and the
            90-token hit is not, because the cut is a strict rank prefix. Example: {'max_tokens': 500, 'counter': 'oml-
            tc/1', 'steering_tokens': 120, 'used_tokens': 420, 'truncated': True, 'hits_omitted': 2, 'next_hit_tokens':
            250}.
        query_digest (str | Unset): SHA-256 of plaintext unless a field says ciphertext.
    """

    steering: list[Memory]
    hits: list[RecallResponseHitsItem]
    budget: RecallBudget | Unset = UNSET
    query_digest: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        steering = []
        for steering_item_data in self.steering:
            steering_item = steering_item_data.to_dict()
            steering.append(steering_item)

        hits = []
        for hits_item_data in self.hits:
            hits_item = hits_item_data.to_dict()
            hits.append(hits_item)

        budget: dict[str, Any] | Unset = UNSET
        if not isinstance(self.budget, Unset):
            budget = self.budget.to_dict()

        query_digest = self.query_digest

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "steering": steering,
                "hits": hits,
            }
        )
        if budget is not UNSET:
            field_dict["budget"] = budget
        if query_digest is not UNSET:
            field_dict["query_digest"] = query_digest

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.memory import Memory
        from ..models.recall_budget import RecallBudget
        from ..models.recall_response_hits_item import RecallResponseHitsItem

        d = dict(src_dict)
        steering = []
        _steering = d.pop("steering")
        for steering_item_data in _steering:
            steering_item = Memory.from_dict(steering_item_data)

            steering.append(steering_item)

        hits = []
        _hits = d.pop("hits")
        for hits_item_data in _hits:
            hits_item = RecallResponseHitsItem.from_dict(hits_item_data)

            hits.append(hits_item)

        _budget = d.pop("budget", UNSET)
        budget: RecallBudget | Unset
        if isinstance(_budget, Unset):
            budget = UNSET
        else:
            budget = RecallBudget.from_dict(_budget)

        query_digest = d.pop("query_digest", UNSET)

        recall_response = cls(
            steering=steering,
            hits=hits,
            budget=budget,
            query_digest=query_digest,
        )

        recall_response.additional_properties = d
        return recall_response

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
