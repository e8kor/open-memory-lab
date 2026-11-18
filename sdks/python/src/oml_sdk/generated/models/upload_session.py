from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.upload_session_purpose import UploadSessionPurpose
from ..models.upload_session_state import UploadSessionState

T = TypeVar("T", bound="UploadSession")


@_attrs_define
class UploadSession:
    """An upload session as the key that opened it reads it; its key is never answered (ADR-0004 d3, as amended
    2026-11-17).

        Attributes:
            upload_id (str):
            purpose (UploadSessionPurpose):
            size (int):
            sha256 (str):
            part_size (int): Always 8388608.
            parts (int):
            received (list[int]):
            missing (list[int]):
            state (UploadSessionState):
            created_at (str):
            expires_at (None | str): Null once bound: a bound upload ends with the verb that bound it.
            completed_at (None | str):
    """

    upload_id: str
    purpose: UploadSessionPurpose
    size: int
    sha256: str
    part_size: int
    parts: int
    received: list[int]
    missing: list[int]
    state: UploadSessionState
    created_at: str
    expires_at: None | str
    completed_at: None | str

    def to_dict(self) -> dict[str, Any]:
        upload_id = self.upload_id

        purpose = self.purpose.value

        size = self.size

        sha256 = self.sha256

        part_size = self.part_size

        parts = self.parts

        received = self.received

        missing = self.missing

        state = self.state.value

        created_at = self.created_at

        expires_at: None | str
        expires_at = self.expires_at

        completed_at: None | str
        completed_at = self.completed_at

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "upload_id": upload_id,
                "purpose": purpose,
                "size": size,
                "sha256": sha256,
                "part_size": part_size,
                "parts": parts,
                "received": received,
                "missing": missing,
                "state": state,
                "created_at": created_at,
                "expires_at": expires_at,
                "completed_at": completed_at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        upload_id = d.pop("upload_id")

        purpose = UploadSessionPurpose(d.pop("purpose"))

        size = d.pop("size")

        sha256 = d.pop("sha256")

        part_size = d.pop("part_size")

        parts = d.pop("parts")

        received = cast(list[int], d.pop("received"))

        missing = cast(list[int], d.pop("missing"))

        state = UploadSessionState(d.pop("state"))

        created_at = d.pop("created_at")

        def _parse_expires_at(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        expires_at = _parse_expires_at(d.pop("expires_at"))

        def _parse_completed_at(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        completed_at = _parse_completed_at(d.pop("completed_at"))

        upload_session = cls(
            upload_id=upload_id,
            purpose=purpose,
            size=size,
            sha256=sha256,
            part_size=part_size,
            parts=parts,
            received=received,
            missing=missing,
            state=state,
            created_at=created_at,
            expires_at=expires_at,
            completed_at=completed_at,
        )

        return upload_session
