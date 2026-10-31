from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="KeyRotateRequest")


@_attrs_define
class KeyRotateRequest:
    """
    Attributes:
        name (str): A new name, in a creation body: the creation grammar. A lowercase RFC 1035 label, 1-63 characters,
            that is not in the id form, not 32 hexadecimal digits and not the derived form, which only rows that predate
            names hold; a creation that sends that form is request.validation. Unique in its namespace, immutable, never
            released; compared byte for byte, never lowercased (docs/api/http-api.md, Names and ids; ADR-0026 d1, d2).
            References use NameRef.
        grace (str | Unset): Bounded grace window before the old key is revoked, e.g. "72h".
    """

    name: str
    grace: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        grace = self.grace

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "name": name,
            }
        )
        if grace is not UNSET:
            field_dict["grace"] = grace

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        name = d.pop("name")

        grace = d.pop("grace", UNSET)

        key_rotate_request = cls(
            name=name,
            grace=grace,
        )

        return key_rotate_request
