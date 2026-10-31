from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ServedDigests")


@_attrs_define
class ServedDigests:
    """
    Attributes:
        body (str | Unset): SHA-256 of plaintext unless a field says ciphertext.
        attachments (list[str] | Unset):
        replaces (str | Unset): SHA-256 of plaintext unless a field says ciphertext.
    """

    body: str | Unset = UNSET
    attachments: list[str] | Unset = UNSET
    replaces: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        body = self.body

        attachments: list[str] | Unset = UNSET
        if not isinstance(self.attachments, Unset):
            attachments = self.attachments

        replaces = self.replaces

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if body is not UNSET:
            field_dict["body"] = body
        if attachments is not UNSET:
            field_dict["attachments"] = attachments
        if replaces is not UNSET:
            field_dict["replaces"] = replaces

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        body = d.pop("body", UNSET)

        attachments = cast(list[str], d.pop("attachments", UNSET))

        replaces = d.pop("replaces", UNSET)

        served_digests = cls(
            body=body,
            attachments=attachments,
            replaces=replaces,
        )

        served_digests.additional_properties = d
        return served_digests

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
