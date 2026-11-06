from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.origin_kind import OriginKind

T = TypeVar("T", bound="Origin")


@_attrs_define
class Origin:
    """What kind of source the text was taken from (ADR-0063 d1, d2): signed in the submission event, immutable per
    version, never inherited by a successor. Omitted on a write, it is recorded manual. chat requires a chat anchor and
    a chat anchor requires chat; either mismatch is request.validation naming /origin/kind.

        Attributes:
            kind (OriginKind):
    """

    kind: OriginKind

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "kind": kind,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        kind = OriginKind(d.pop("kind"))

        origin = cls(
            kind=kind,
        )

        return origin
