from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.replay_report_pipelines_item import ReplayReportPipelinesItem


T = TypeVar("T", bound="ReplayReport")


@_attrs_define
class ReplayReport:
    """The real-usage replay's scores, numbers and counts alone (ADR-0054 d11).

    Attributes:
        labelled (int):
        capped (bool): The replay stopped short: past 500 labelled recalls the newest 500 ran, or it reached its
            60-second deadline.
        skipped_revoked (int):
        window_days (int):
        pipelines (list[ReplayReportPipelinesItem]):
    """

    labelled: int
    capped: bool
    skipped_revoked: int
    window_days: int
    pipelines: list[ReplayReportPipelinesItem]

    def to_dict(self) -> dict[str, Any]:
        labelled = self.labelled

        capped = self.capped

        skipped_revoked = self.skipped_revoked

        window_days = self.window_days

        pipelines = []
        for pipelines_item_data in self.pipelines:
            pipelines_item = pipelines_item_data.to_dict()
            pipelines.append(pipelines_item)

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "labelled": labelled,
                "capped": capped,
                "skipped_revoked": skipped_revoked,
                "window_days": window_days,
                "pipelines": pipelines,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.replay_report_pipelines_item import ReplayReportPipelinesItem

        d = dict(src_dict)
        labelled = d.pop("labelled")

        capped = d.pop("capped")

        skipped_revoked = d.pop("skipped_revoked")

        window_days = d.pop("window_days")

        pipelines = []
        _pipelines = d.pop("pipelines")
        for pipelines_item_data in _pipelines:
            pipelines_item = ReplayReportPipelinesItem.from_dict(pipelines_item_data)

            pipelines.append(pipelines_item)

        replay_report = cls(
            labelled=labelled,
            capped=capped,
            skipped_revoked=skipped_revoked,
            window_days=window_days,
            pipelines=pipelines,
        )

        return replay_report
