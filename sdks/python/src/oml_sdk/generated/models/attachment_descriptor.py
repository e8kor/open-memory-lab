from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="AttachmentDescriptor")


@_attrs_define
class AttachmentDescriptor:
    """
    Attributes:
        digest (str | Unset): SHA-256 of plaintext unless a field says ciphertext.
        media_type (str | Unset):
        size_bytes (int | Unset):
    """

    digest: str | Unset = UNSET
    media_type: str | Unset = UNSET
    size_bytes: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        digest = self.digest

        media_type = self.media_type

        size_bytes = self.size_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if digest is not UNSET:
            field_dict["digest"] = digest
        if media_type is not UNSET:
            field_dict["media_type"] = media_type
        if size_bytes is not UNSET:
            field_dict["size_bytes"] = size_bytes

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        digest = d.pop("digest", UNSET)

        media_type = d.pop("media_type", UNSET)

        size_bytes = d.pop("size_bytes", UNSET)

        attachment_descriptor = cls(
            digest=digest,
            media_type=media_type,
            size_bytes=size_bytes,
        )

        attachment_descriptor.additional_properties = d
        return attachment_descriptor

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
