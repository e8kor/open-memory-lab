from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define

from ..models.admin_fix_body_act import AdminFixBodyAct
from ..types import UNSET, Unset

T = TypeVar("T", bound="AdminFixBody")


@_attrs_define
class AdminFixBody:
    """
    Attributes:
        act (AdminFixBodyAct):
        target (UUID): The version the act names.
        reason (str):
        body (str | Unset): An edit's new body; refused on every other act.
        title (str | Unset): An edit's new title, where its format takes one; refused on every other act.
    """

    act: AdminFixBodyAct
    target: UUID
    reason: str
    body: str | Unset = UNSET
    title: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        act = self.act.value

        target = str(self.target)

        reason = self.reason

        body = self.body

        title = self.title

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "act": act,
                "target": target,
                "reason": reason,
            }
        )
        if body is not UNSET:
            field_dict["body"] = body
        if title is not UNSET:
            field_dict["title"] = title

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        act = AdminFixBodyAct(d.pop("act"))

        target = UUID(d.pop("target"))

        reason = d.pop("reason")

        body = d.pop("body", UNSET)

        title = d.pop("title", UNSET)

        admin_fix_body = cls(
            act=act,
            target=target,
            reason=reason,
            body=body,
            title=title,
        )

        return admin_fix_body
