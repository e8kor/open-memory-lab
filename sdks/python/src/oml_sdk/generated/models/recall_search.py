from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.recall_search_match import RecallSearchMatch
from ..models.recall_search_pipeline import RecallSearchPipeline

T = TypeVar("T", bound="RecallSearch")


@_attrs_define
class RecallSearch:
    """What ran and how the query was read (ADR-0049 d5, d6).

    Attributes:
        pipeline (RecallSearchPipeline):
        match (RecallSearchMatch):
        terms (list[str]):
        fallback (None | str): any_term when match all found no row with every word and the any-word hits were served;
            null otherwise.
        did_you_mean (list[str]): Spellings from rows the key may read, from a pipeline with a trigram list, when no hit
            holds every word; else empty.
    """

    pipeline: RecallSearchPipeline
    match: RecallSearchMatch
    terms: list[str]
    fallback: None | str
    did_you_mean: list[str]

    def to_dict(self) -> dict[str, Any]:
        pipeline = self.pipeline.value

        match = self.match.value

        terms = self.terms

        fallback: None | str
        fallback = self.fallback

        did_you_mean = self.did_you_mean

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "pipeline": pipeline,
                "match": match,
                "terms": terms,
                "fallback": fallback,
                "did_you_mean": did_you_mean,
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

        recall_search = cls(
            pipeline=pipeline,
            match=match,
            terms=terms,
            fallback=fallback,
            did_you_mean=did_you_mean,
        )

        return recall_search
