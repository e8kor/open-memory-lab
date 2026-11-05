from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="ReplayReportPipelinesItem")


@_attrs_define
class ReplayReportPipelinesItem:
    """
    Attributes:
        pipeline (str):
        queries (int):
        success_at_1 (float):
        success_at_5 (float):
        ndcg_at_10 (float):
        recall_at_10 (float):
    """

    pipeline: str
    queries: int
    success_at_1: float
    success_at_5: float
    ndcg_at_10: float
    recall_at_10: float

    def to_dict(self) -> dict[str, Any]:
        pipeline = self.pipeline

        queries = self.queries

        success_at_1 = self.success_at_1

        success_at_5 = self.success_at_5

        ndcg_at_10 = self.ndcg_at_10

        recall_at_10 = self.recall_at_10

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "pipeline": pipeline,
                "queries": queries,
                "success_at_1": success_at_1,
                "success_at_5": success_at_5,
                "ndcg_at_10": ndcg_at_10,
                "recall_at_10": recall_at_10,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        pipeline = d.pop("pipeline")

        queries = d.pop("queries")

        success_at_1 = d.pop("success_at_1")

        success_at_5 = d.pop("success_at_5")

        ndcg_at_10 = d.pop("ndcg_at_10")

        recall_at_10 = d.pop("recall_at_10")

        replay_report_pipelines_item = cls(
            pipeline=pipeline,
            queries=queries,
            success_at_1=success_at_1,
            success_at_5=success_at_5,
            ndcg_at_10=ndcg_at_10,
            recall_at_10=recall_at_10,
        )

        return replay_report_pipelines_item
