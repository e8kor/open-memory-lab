from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.recall_search_widen_sources import RecallSearchWidenSources

T = TypeVar("T", bound="RecallSearchWiden")


@_attrs_define
class RecallSearchWiden:
    """On a sources scope answer, when a workarea the key reads outside the scope holds the query's words at least twice as
    often per row as the scope, on at least two of its rows: a retry with sources wide would search it. Read off a
    signal the server builds in the background, at most 300 seconds old and under the key's grants as they are now; a
    scope answer without such a signal carries no widen. It names no workarea (ADR-0072 d8).

        Attributes:
            sources (RecallSearchWidenSources):
            candidates (int): The workareas outside the scope that hold a query word.
            matches_at_least (int): A lower bound on their rows holding a query word: the largest count of one word, summed
                over them.
    """

    sources: RecallSearchWidenSources
    candidates: int
    matches_at_least: int

    def to_dict(self) -> dict[str, Any]:
        sources = self.sources.value

        candidates = self.candidates

        matches_at_least = self.matches_at_least

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "sources": sources,
                "candidates": candidates,
                "matches_at_least": matches_at_least,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        sources = RecallSearchWidenSources(d.pop("sources"))

        candidates = d.pop("candidates")

        matches_at_least = d.pop("matches_at_least")

        recall_search_widen = cls(
            sources=sources,
            candidates=candidates,
            matches_at_least=matches_at_least,
        )

        return recall_search_widen
