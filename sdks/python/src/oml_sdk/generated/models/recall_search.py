from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.recall_search_access import RecallSearchAccess
from ..models.recall_search_match import RecallSearchMatch
from ..models.recall_search_pipeline import RecallSearchPipeline

T = TypeVar("T", bound="RecallSearch")


@_attrs_define
class RecallSearch:
    """What ran and how the query was read (ADR-0049 d5, d6; ADR-0052 d11).

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
    """

    pipeline: RecallSearchPipeline
    match: RecallSearchMatch
    terms: list[str]
    fallback: None | str
    did_you_mean: list[str]
    access: RecallSearchAccess
    pruned: list[str]

    def to_dict(self) -> dict[str, Any]:
        pipeline = self.pipeline.value

        match = self.match.value

        terms = self.terms

        fallback: None | str
        fallback = self.fallback

        did_you_mean = self.did_you_mean

        access = self.access.value

        pruned = self.pruned

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
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
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

        recall_search = cls(
            pipeline=pipeline,
            match=match,
            terms=terms,
            fallback=fallback,
            did_you_mean=did_you_mean,
            access=access,
            pruned=pruned,
        )

        return recall_search
