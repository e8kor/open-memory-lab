from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.convert_self_body_to import ConvertSelfBodyTo

T = TypeVar("T", bound="ConvertSelfBody")


@_attrs_define
class ConvertSelfBody:
    """
    Attributes:
        to (ConvertSelfBodyTo):
    """

    to: ConvertSelfBodyTo

    def to_dict(self) -> dict[str, Any]:
        to = self.to.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "to": to,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        to = ConvertSelfBodyTo(d.pop("to"))

        convert_self_body = cls(
            to=to,
        )

        return convert_self_body
