from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.rescan_report_by_rule_item_action import RescanReportByRuleItemAction

T = TypeVar("T", bound="RescanReportByRuleItem")


@_attrs_define
class RescanReportByRuleItem:
    """
    Attributes:
        class_ (str):
        rule (str):
        action (RescanReportByRuleItemAction):
        count (int):
    """

    class_: str
    rule: str
    action: RescanReportByRuleItemAction
    count: int

    def to_dict(self) -> dict[str, Any]:
        class_ = self.class_

        rule = self.rule

        action = self.action.value

        count = self.count

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "class": class_,
                "rule": rule,
                "action": action,
                "count": count,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        class_ = d.pop("class")

        rule = d.pop("rule")

        action = RescanReportByRuleItemAction(d.pop("action"))

        count = d.pop("count")

        rescan_report_by_rule_item = cls(
            class_=class_,
            rule=rule,
            action=action,
            count=count,
        )

        return rescan_report_by_rule_item
