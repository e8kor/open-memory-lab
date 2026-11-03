from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.upload_session_state import UploadSessionState
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.upload_session_parts_item import UploadSessionPartsItem


T = TypeVar("T", bound="UploadSession")


@_attrs_define
class UploadSession:
    """
    Attributes:
        session_id (str):
        expected_digest (str): SHA-256 of plaintext unless a field says ciphertext.
        state (UploadSessionState):
        size_bytes (int | Unset):
        media_type (str | Unset):
        parts (list[UploadSessionPartsItem] | Unset):
        expires_at (datetime.datetime | Unset):
    """

    session_id: str
    expected_digest: str
    state: UploadSessionState
    size_bytes: int | Unset = UNSET
    media_type: str | Unset = UNSET
    parts: list[UploadSessionPartsItem] | Unset = UNSET
    expires_at: datetime.datetime | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        session_id = self.session_id

        expected_digest = self.expected_digest

        state = self.state.value

        size_bytes = self.size_bytes

        media_type = self.media_type

        parts: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.parts, Unset):
            parts = []
            for parts_item_data in self.parts:
                parts_item = parts_item_data.to_dict()
                parts.append(parts_item)

        expires_at: str | Unset = UNSET
        if not isinstance(self.expires_at, Unset):
            expires_at = self.expires_at.isoformat()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "session_id": session_id,
                "expected_digest": expected_digest,
                "state": state,
            }
        )
        if size_bytes is not UNSET:
            field_dict["size_bytes"] = size_bytes
        if media_type is not UNSET:
            field_dict["media_type"] = media_type
        if parts is not UNSET:
            field_dict["parts"] = parts
        if expires_at is not UNSET:
            field_dict["expires_at"] = expires_at

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.upload_session_parts_item import UploadSessionPartsItem

        d = dict(src_dict)
        session_id = d.pop("session_id")

        expected_digest = d.pop("expected_digest")

        state = UploadSessionState(d.pop("state"))

        size_bytes = d.pop("size_bytes", UNSET)

        media_type = d.pop("media_type", UNSET)

        _parts = d.pop("parts", UNSET)
        parts: list[UploadSessionPartsItem] | Unset = UNSET
        if _parts is not UNSET:
            parts = []
            for parts_item_data in _parts:
                parts_item = UploadSessionPartsItem.from_dict(parts_item_data)

                parts.append(parts_item)

        _expires_at = d.pop("expires_at", UNSET)
        expires_at: datetime.datetime | Unset
        if isinstance(_expires_at, Unset):
            expires_at = UNSET
        else:
            expires_at = datetime.datetime.fromisoformat(_expires_at)

        upload_session = cls(
            session_id=session_id,
            expected_digest=expected_digest,
            state=state,
            size_bytes=size_bytes,
            media_type=media_type,
            parts=parts,
            expires_at=expires_at,
        )

        upload_session.additional_properties = d
        return upload_session

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
