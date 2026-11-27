from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.notice_kind import NoticeKind

if TYPE_CHECKING:
    from ..models.notice_detail import NoticeDetail


T = TypeVar("T", bound="Notice")


@_attrs_define
class Notice:
    """One in-product notice to one person (ADR-0097 d20).

    Attributes:
        notice_id (UUID):
        kind (NoticeKind):
        act_id (None | UUID): The admin act the notice tells about.
        detail (NoticeDetail): The act's kind, its target and the person the acting key counted as.
        at (datetime.datetime):
        dismissed_at (datetime.datetime | None):
    """

    notice_id: UUID
    kind: NoticeKind
    act_id: None | UUID
    detail: NoticeDetail
    at: datetime.datetime
    dismissed_at: datetime.datetime | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        notice_id = str(self.notice_id)

        kind = self.kind.value

        act_id: None | str
        if isinstance(self.act_id, UUID):
            act_id = str(self.act_id)
        else:
            act_id = self.act_id

        detail = self.detail.to_dict()

        at = self.at.isoformat()

        dismissed_at: None | str
        if isinstance(self.dismissed_at, datetime.datetime):
            dismissed_at = self.dismissed_at.isoformat()
        else:
            dismissed_at = self.dismissed_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "notice_id": notice_id,
                "kind": kind,
                "act_id": act_id,
                "detail": detail,
                "at": at,
                "dismissed_at": dismissed_at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.notice_detail import NoticeDetail

        d = dict(src_dict)
        notice_id = UUID(d.pop("notice_id"))

        kind = NoticeKind(d.pop("kind"))

        def _parse_act_id(data: object) -> None | UUID:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                act_id_type_0 = UUID(data)

                return act_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | UUID, data)

        act_id = _parse_act_id(d.pop("act_id"))

        detail = NoticeDetail.from_dict(d.pop("detail"))

        at = datetime.datetime.fromisoformat(d.pop("at"))

        def _parse_dismissed_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                dismissed_at_type_0 = datetime.datetime.fromisoformat(data)

                return dismissed_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        dismissed_at = _parse_dismissed_at(d.pop("dismissed_at"))

        notice = cls(
            notice_id=notice_id,
            kind=kind,
            act_id=act_id,
            detail=detail,
            at=at,
            dismissed_at=dismissed_at,
        )

        notice.additional_properties = d
        return notice

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
