from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.rescan_report_findings_item_action import RescanReportFindingsItemAction
from ..types import UNSET, Unset

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
        field (str | Unset): The field the findings list stores it under: /body, /title, /body/ and a decoded view's
            path, or an anchor field (ADR-0074 d23).
        origin (None | str | Unset): The version's origin kind, or null for one stored without: the dry run's rates per
            origin (ADR-0074 d18).
    """

    artifact_id: str
    workarea_id: str
    version: int
    state: str
    class_: str
    rule: str
    action: RescanReportFindingsItemAction
    at: str
    field: str | Unset = UNSET
    origin: None | str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        artifact_id = self.artifact_id

        workarea_id = self.workarea_id

        version = self.version

        state = self.state

        class_ = self.class_

        rule = self.rule

        action = self.action.value

        at = self.at

        field = self.field

        origin: None | str | Unset
        if isinstance(self.origin, Unset):
            origin = UNSET
        else:
            origin = self.origin

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
        if field is not UNSET:
            field_dict["field"] = field
        if origin is not UNSET:
            field_dict["origin"] = origin

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

        field = d.pop("field", UNSET)

        def _parse_origin(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        origin = _parse_origin(d.pop("origin", UNSET))

        rescan_report_findings_item = cls(
            artifact_id=artifact_id,
            workarea_id=workarea_id,
            version=version,
            state=state,
            class_=class_,
            rule=rule,
            action=action,
            at=at,
            field=field,
            origin=origin,
        )

        return rescan_report_findings_item
