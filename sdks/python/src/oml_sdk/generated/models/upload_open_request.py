from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.upload_open_request_purpose import UploadOpenRequestPurpose

T = TypeVar("T", bound="UploadOpenRequest")


@_attrs_define
class UploadOpenRequest:
    """An upload session to open (ADR-0004 d3, as amended 2026-11-17).

    Attributes:
        purpose (UploadOpenRequestPurpose):
        size (int): The plaintext's size in bytes, at most uploads.max_bytes.
        sha256 (str): The plaintext's SHA-256, 64 lowercase hex digits.
    """

    purpose: UploadOpenRequestPurpose
    size: int
    sha256: str

    def to_dict(self) -> dict[str, Any]:
        purpose = self.purpose.value

        size = self.size

        sha256 = self.sha256

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "purpose": purpose,
                "size": size,
                "sha256": sha256,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        purpose = UploadOpenRequestPurpose(d.pop("purpose"))

        size = d.pop("size")

        sha256 = d.pop("sha256")

        upload_open_request = cls(
            purpose=purpose,
            size=size,
            sha256=sha256,
        )

        return upload_open_request
