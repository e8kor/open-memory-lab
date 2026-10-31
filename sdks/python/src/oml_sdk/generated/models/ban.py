from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.ban_subject_kind import BanSubjectKind
from ..types import UNSET, Unset

T = TypeVar("T", bound="Ban")


@_attrs_define
class Ban:
    """
    Attributes:
        ban_id (str):
        subject_kind (BanSubjectKind):
        subject (str):
        reason (str | Unset):
        imposed_by_key_id (str | Unset):
        imposed_at (datetime.datetime | Unset):
        lifted_by_key_id (None | str | Unset):
        lifted_at (datetime.datetime | None | Unset):
    """

    ban_id: str
    subject_kind: BanSubjectKind
    subject: str
    reason: str | Unset = UNSET
    imposed_by_key_id: str | Unset = UNSET
    imposed_at: datetime.datetime | Unset = UNSET
    lifted_by_key_id: None | str | Unset = UNSET
    lifted_at: datetime.datetime | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        ban_id = self.ban_id

        subject_kind = self.subject_kind.value

        subject = self.subject

        reason = self.reason

        imposed_by_key_id = self.imposed_by_key_id

        imposed_at: str | Unset = UNSET
        if not isinstance(self.imposed_at, Unset):
            imposed_at = self.imposed_at.isoformat()

        lifted_by_key_id: None | str | Unset
        if isinstance(self.lifted_by_key_id, Unset):
            lifted_by_key_id = UNSET
        else:
            lifted_by_key_id = self.lifted_by_key_id

        lifted_at: None | str | Unset
        if isinstance(self.lifted_at, Unset):
            lifted_at = UNSET
        elif isinstance(self.lifted_at, datetime.datetime):
            lifted_at = self.lifted_at.isoformat()
        else:
            lifted_at = self.lifted_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "ban_id": ban_id,
                "subject_kind": subject_kind,
                "subject": subject,
            }
        )
        if reason is not UNSET:
            field_dict["reason"] = reason
        if imposed_by_key_id is not UNSET:
            field_dict["imposed_by_key_id"] = imposed_by_key_id
        if imposed_at is not UNSET:
            field_dict["imposed_at"] = imposed_at
        if lifted_by_key_id is not UNSET:
            field_dict["lifted_by_key_id"] = lifted_by_key_id
        if lifted_at is not UNSET:
            field_dict["lifted_at"] = lifted_at

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        ban_id = d.pop("ban_id")

        subject_kind = BanSubjectKind(d.pop("subject_kind"))

        subject = d.pop("subject")

        reason = d.pop("reason", UNSET)

        imposed_by_key_id = d.pop("imposed_by_key_id", UNSET)

        _imposed_at = d.pop("imposed_at", UNSET)
        imposed_at: datetime.datetime | Unset
        if isinstance(_imposed_at, Unset):
            imposed_at = UNSET
        else:
            imposed_at = datetime.datetime.fromisoformat(_imposed_at)

        def _parse_lifted_by_key_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        lifted_by_key_id = _parse_lifted_by_key_id(d.pop("lifted_by_key_id", UNSET))

        def _parse_lifted_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                lifted_at_type_0 = datetime.datetime.fromisoformat(data)

                return lifted_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        lifted_at = _parse_lifted_at(d.pop("lifted_at", UNSET))

        ban = cls(
            ban_id=ban_id,
            subject_kind=subject_kind,
            subject=subject,
            reason=reason,
            imposed_by_key_id=imposed_by_key_id,
            imposed_at=imposed_at,
            lifted_by_key_id=lifted_by_key_id,
            lifted_at=lifted_at,
        )

        ban.additional_properties = d
        return ban

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
