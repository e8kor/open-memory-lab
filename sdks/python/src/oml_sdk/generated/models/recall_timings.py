from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.recall_timings_stages_ms import RecallTimingsStagesMs


T = TypeVar("T", bound="RecallTimings")


@_attrs_define
class RecallTimings:
    """Each stage's milliseconds, in the order the stages ran, and their sum (ADR-0049 d13). Present only when the request
    sent timings true, which a deployment serves only with search.timings on.

        Attributes:
            stages_ms (RecallTimingsStagesMs): Keys from the closed set scope, retrieve_keyword_all, retrieve_keyword_any,
                retrieve_bm25, retrieve_trigram, retrieve_bm25_postings, retrieve_sparse, fuse, rerank, suggest, hydrate, cut; a
                stage that did not run is absent.
            total_ms (float):
    """

    stages_ms: RecallTimingsStagesMs
    total_ms: float

    def to_dict(self) -> dict[str, Any]:
        stages_ms = self.stages_ms.to_dict()

        total_ms = self.total_ms

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "stages_ms": stages_ms,
                "total_ms": total_ms,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.recall_timings_stages_ms import RecallTimingsStagesMs

        d = dict(src_dict)
        stages_ms = RecallTimingsStagesMs.from_dict(d.pop("stages_ms"))

        total_ms = d.pop("total_ms")

        recall_timings = cls(
            stages_ms=stages_ms,
            total_ms=total_ms,
        )

        return recall_timings
