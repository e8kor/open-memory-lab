from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.recall_matched_via_item import RecallMatchedViaItem
from ..types import UNSET, Unset

T = TypeVar("T", bound="RecallMatched")


@_attrs_define
class RecallMatched:
    """Why a hit matched (ADR-0049 d6).

    Attributes:
        terms (list[str]): The query words the row holds, as the pipeline reads words.
        of (int): How many words the query had.
        via (list[RecallMatchedViaItem]): The retrievers whose lists held the row.
        rerank (float | None | Unset): The reranker's relevance of the row to the query, when a reranking pipeline
            scored it; the hit's score is then this relevance (ADR-0053 d7).
        rank_before (int | None | Unset): The row's rank in the first stage's list, from 1, when the reranker scored it
            (ADR-0053 d7).
    """

    terms: list[str]
    of: int
    via: list[RecallMatchedViaItem]
    rerank: float | None | Unset = UNSET
    rank_before: int | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        terms = self.terms

        of = self.of

        via = []
        for via_item_data in self.via:
            via_item = via_item_data.value
            via.append(via_item)

        rerank: float | None | Unset
        if isinstance(self.rerank, Unset):
            rerank = UNSET
        else:
            rerank = self.rerank

        rank_before: int | None | Unset
        if isinstance(self.rank_before, Unset):
            rank_before = UNSET
        else:
            rank_before = self.rank_before

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "terms": terms,
                "of": of,
                "via": via,
            }
        )
        if rerank is not UNSET:
            field_dict["rerank"] = rerank
        if rank_before is not UNSET:
            field_dict["rank_before"] = rank_before

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        terms = cast(list[str], d.pop("terms"))

        of = d.pop("of")

        via = []
        _via = d.pop("via")
        for via_item_data in _via:
            via_item = RecallMatchedViaItem(via_item_data)

            via.append(via_item)

        def _parse_rerank(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        rerank = _parse_rerank(d.pop("rerank", UNSET))

        def _parse_rank_before(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        rank_before = _parse_rank_before(d.pop("rank_before", UNSET))

        recall_matched = cls(
            terms=terms,
            of=of,
            via=via,
            rerank=rerank,
            rank_before=rank_before,
        )

        return recall_matched
