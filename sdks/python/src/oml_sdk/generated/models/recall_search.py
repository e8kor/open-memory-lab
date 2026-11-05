from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.recall_search_access import RecallSearchAccess
from ..models.recall_search_match import RecallSearchMatch
from ..models.recall_search_pipeline import RecallSearchPipeline
from ..models.rerank_outcome import RerankOutcome
from ..models.rerank_reason import RerankReason
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.recall_search_window_type_0 import RecallSearchWindowType0


T = TypeVar("T", bound="RecallSearch")


@_attrs_define
class RecallSearch:
    """What ran and how the query was read (ADR-0049 d5, d6; ADR-0052 d11; ADR-0053 d5-d7).

    Attributes:
        pipeline (RecallSearchPipeline):
        match (RecallSearchMatch):
        terms (list[str]):
        fallback (None | str): any_term when match all found no row with every word and the any-word hits were served;
            null otherwise.
        did_you_mean (list[str]): Spellings from rows the key may read, from a pipeline with a trigram list, when no hit
            holds every word; else empty.
        access (RecallSearchAccess): postings when every list was read from integer postings by their index; scan when
            any list scanned the rows in scope, as every pipeline but bm25-postings does, and as bm25-postings does when
            every query word was pruned (ADR-0052 d6).
        pruned (list[str]): The query words left out of the postings read because they are frequent among the rows the
            key may read; a hit the other words reach is still scored with them, and a memory holding only them is not
            found. Empty for a scan (ADR-0052 d6, d11).
        rerank (None | RerankOutcome): applied when a reranking pipeline reordered the head of its first stage; skipped
            when the reranker was busy, late or failed and the first stage's order was served; null when the pipeline does
            not rerank or found nothing.
        rerank_reason (None | RerankReason): Why the rerank was skipped; null otherwise.
        floor (float | None): The relevance floor applied to this answer (search.rerank.floor), only when the rerank was
            applied; null when none was.
        below_floor (int): How many of the first k candidates the floor withheld; hits empty with below_floor above 0
            means nothing scored above the floor (ADR-0053 d6).
        window (None | RecallSearchWindowType0 | Unset): The date window read from the query by rule oml-when/1,
            anchored to the recall's now; null when the query names no date (proposed ADR-0059 d6).
    """

    pipeline: RecallSearchPipeline
    match: RecallSearchMatch
    terms: list[str]
    fallback: None | str
    did_you_mean: list[str]
    access: RecallSearchAccess
    pruned: list[str]
    rerank: None | RerankOutcome
    rerank_reason: None | RerankReason
    floor: float | None
    below_floor: int
    window: None | RecallSearchWindowType0 | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.recall_search_window_type_0 import RecallSearchWindowType0

        pipeline = self.pipeline.value

        match = self.match.value

        terms = self.terms

        fallback: None | str
        fallback = self.fallback

        did_you_mean = self.did_you_mean

        access = self.access.value

        pruned = self.pruned

        rerank: None | str
        if isinstance(self.rerank, RerankOutcome):
            rerank = self.rerank.value
        else:
            rerank = self.rerank

        rerank_reason: None | str
        if isinstance(self.rerank_reason, RerankReason):
            rerank_reason = self.rerank_reason.value
        else:
            rerank_reason = self.rerank_reason

        floor: float | None
        floor = self.floor

        below_floor = self.below_floor

        window: dict[str, Any] | None | Unset
        if isinstance(self.window, Unset):
            window = UNSET
        elif isinstance(self.window, RecallSearchWindowType0):
            window = self.window.to_dict()
        else:
            window = self.window

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "pipeline": pipeline,
                "match": match,
                "terms": terms,
                "fallback": fallback,
                "did_you_mean": did_you_mean,
                "access": access,
                "pruned": pruned,
                "rerank": rerank,
                "rerank_reason": rerank_reason,
                "floor": floor,
                "below_floor": below_floor,
            }
        )
        if window is not UNSET:
            field_dict["window"] = window

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.recall_search_window_type_0 import RecallSearchWindowType0

        d = dict(src_dict)
        pipeline = RecallSearchPipeline(d.pop("pipeline"))

        match = RecallSearchMatch(d.pop("match"))

        terms = cast(list[str], d.pop("terms"))

        def _parse_fallback(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        fallback = _parse_fallback(d.pop("fallback"))

        did_you_mean = cast(list[str], d.pop("did_you_mean"))

        access = RecallSearchAccess(d.pop("access"))

        pruned = cast(list[str], d.pop("pruned"))

        def _parse_rerank(data: object) -> None | RerankOutcome:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                rerank_type_0 = RerankOutcome(data)

                return rerank_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | RerankOutcome, data)

        rerank = _parse_rerank(d.pop("rerank"))

        def _parse_rerank_reason(data: object) -> None | RerankReason:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                rerank_reason_type_0 = RerankReason(data)

                return rerank_reason_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | RerankReason, data)

        rerank_reason = _parse_rerank_reason(d.pop("rerank_reason"))

        def _parse_floor(data: object) -> float | None:
            if data is None:
                return data
            return cast(float | None, data)

        floor = _parse_floor(d.pop("floor"))

        below_floor = d.pop("below_floor")

        def _parse_window(data: object) -> None | RecallSearchWindowType0 | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                window_type_0 = RecallSearchWindowType0.from_dict(data)

                return window_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | RecallSearchWindowType0 | Unset, data)

        window = _parse_window(d.pop("window", UNSET))

        recall_search = cls(
            pipeline=pipeline,
            match=match,
            terms=terms,
            fallback=fallback,
            did_you_mean=did_you_mean,
            access=access,
            pruned=pruned,
            rerank=rerank,
            rerank_reason=rerank_reason,
            floor=floor,
            below_floor=below_floor,
            window=window,
        )

        return recall_search
