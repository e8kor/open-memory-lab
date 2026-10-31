from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.ban_request_subject_kind import BanRequestSubjectKind
from ..types import UNSET, Unset

T = TypeVar("T", bound="BanRequest")


@_attrs_define
class BanRequest:
    """
    Attributes:
        subject_kind (BanRequestSubjectKind):
        subject (str):
        reason (str | Unset):
        dry_run (bool | Unset): Returns the reach preview without journalling. Default: False.
    """

    subject_kind: BanRequestSubjectKind
    subject: str
    reason: str | Unset = UNSET
    dry_run: bool | Unset = False
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        subject_kind = self.subject_kind.value

        subject = self.subject

        reason = self.reason

        dry_run = self.dry_run

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "subject_kind": subject_kind,
                "subject": subject,
            }
        )
        if reason is not UNSET:
            field_dict["reason"] = reason
        if dry_run is not UNSET:
            field_dict["dry_run"] = dry_run

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        subject_kind = BanRequestSubjectKind(d.pop("subject_kind"))

        subject = d.pop("subject")

        reason = d.pop("reason", UNSET)

        dry_run = d.pop("dry_run", UNSET)

        ban_request = cls(
            subject_kind=subject_kind,
            subject=subject,
            reason=reason,
            dry_run=dry_run,
        )

        ban_request.additional_properties = d
        return ban_request

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
