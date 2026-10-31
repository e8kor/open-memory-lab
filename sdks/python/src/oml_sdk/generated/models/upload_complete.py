from __future__ import annotations

from collections.abc import Mapping
from typing import (
    Any,
    Literal,
    Self,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="UploadComplete")


@_attrs_define
class UploadComplete:
    """
    Attributes:
        digest (str): SHA-256 of plaintext unless a field says ciphertext.
        state (Literal['complete']):
        ciphertext_digest (str | Unset): SHA-256 of plaintext unless a field says ciphertext.
    """

    digest: str
    state: Literal["complete"]
    ciphertext_digest: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        digest = self.digest

        state = self.state

        ciphertext_digest = self.ciphertext_digest

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "digest": digest,
                "state": state,
            }
        )
        if ciphertext_digest is not UNSET:
            field_dict["ciphertext_digest"] = ciphertext_digest

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        digest = d.pop("digest")

        state = cast(Literal["complete"], d.pop("state"))
        if state != "complete":
            raise ValueError(f"state must match const 'complete', got '{state}'")

        ciphertext_digest = d.pop("ciphertext_digest", UNSET)

        upload_complete = cls(
            digest=digest,
            state=state,
            ciphertext_digest=ciphertext_digest,
        )

        upload_complete.additional_properties = d
        return upload_complete

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
