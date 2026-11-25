from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="RotateKeyBody")


@_attrs_define
class RotateKeyBody:
    """
    Attributes:
        grace_seconds (int | Unset): How long the previous value keeps authenticating; omitted or 0, it stops at once.
    """

    grace_seconds: int | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        grace_seconds = self.grace_seconds

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if grace_seconds is not UNSET:
            field_dict["grace_seconds"] = grace_seconds

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        grace_seconds = d.pop("grace_seconds", UNSET)

        rotate_key_body = cls(
            grace_seconds=grace_seconds,
        )

        return rotate_key_body
