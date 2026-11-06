from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.rescan_report_findings_item_action import RescanReportFindingsItemAction

T = TypeVar("T", bound="RescanReportFindingsItem")


@_attrs_define
class RescanReportFindingsItem:
    """
    Attributes:
        artifact_id (str):
        workarea_id (str):
        version (int):
        state (str):
        class_ (str):
        rule (str):
        action (RescanReportFindingsItemAction):
        at (str): The field and line, /body line 3.
    """

    artifact_id: str
    workarea_id: str
    version: int
    state: str
    class_: str
    rule: str
    action: RescanReportFindingsItemAction
    at: str

    def to_dict(self) -> dict[str, Any]:
        artifact_id = self.artifact_id

        workarea_id = self.workarea_id

        version = self.version

        state = self.state

        class_ = self.class_

        rule = self.rule

        action = self.action.value

        at = self.at

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "artifact_id": artifact_id,
                "workarea_id": workarea_id,
                "version": version,
                "state": state,
                "class": class_,
                "rule": rule,
                "action": action,
                "at": at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        artifact_id = d.pop("artifact_id")

        workarea_id = d.pop("workarea_id")

        version = d.pop("version")

        state = d.pop("state")

        class_ = d.pop("class")

        rule = d.pop("rule")

        action = RescanReportFindingsItemAction(d.pop("action"))

        at = d.pop("at")

        rescan_report_findings_item = cls(
            artifact_id=artifact_id,
            workarea_id=workarea_id,
            version=version,
            state=state,
            class_=class_,
            rule=rule,
            action=action,
            at=at,
        )

        return rescan_report_findings_item
