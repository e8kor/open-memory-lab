from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="UploadSessionCreate")


@_attrs_define
class UploadSessionCreate:
    """
    Attributes:
        expected_digest (str): SHA-256 of plaintext unless a field says ciphertext.
        size_bytes (int): Attachments are at most 64 MiB (http-api.md Limits).
        media_type (str):
    """

    expected_digest: str
    size_bytes: int
    media_type: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        expected_digest = self.expected_digest

        size_bytes = self.size_bytes

        media_type = self.media_type

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "expected_digest": expected_digest,
                "size_bytes": size_bytes,
                "media_type": media_type,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        expected_digest = d.pop("expected_digest")

        size_bytes = d.pop("size_bytes")

        media_type = d.pop("media_type")

        upload_session_create = cls(
            expected_digest=expected_digest,
            size_bytes=size_bytes,
            media_type=media_type,
        )

        upload_session_create.additional_properties = d
        return upload_session_create

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
