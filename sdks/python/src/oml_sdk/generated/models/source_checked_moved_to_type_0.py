from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="SourceCheckedMovedToType0")


@_attrs_define
class SourceCheckedMovedToType0:
    """
    Attributes:
        path (str):
        fragment (None | str):
    """

    path: str
    fragment: None | str

    def to_dict(self) -> dict[str, Any]:
        path = self.path

        fragment: None | str
        fragment = self.fragment

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "path": path,
                "fragment": fragment,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        path = d.pop("path")

        def _parse_fragment(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        fragment = _parse_fragment(d.pop("fragment"))

        source_checked_moved_to_type_0 = cls(
            path=path,
            fragment=fragment,
        )

        return source_checked_moved_to_type_0
