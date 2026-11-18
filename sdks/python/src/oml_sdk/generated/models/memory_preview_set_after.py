from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="MemoryPreviewSetAfter")


@_attrs_define
class MemoryPreviewSetAfter:
    """A guardrail's set size at its workspace once the version is approved, the approved rules an agent there loads,
    against the set cap (ADR-0037 d7).

        Attributes:
            size (int):
            cap (int):
    """

    size: int
    cap: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        size = self.size

        cap = self.cap

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "size": size,
                "cap": cap,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        size = d.pop("size")

        cap = d.pop("cap")

        memory_preview_set_after = cls(
            size=size,
            cap=cap,
        )

        memory_preview_set_after.additional_properties = d
        return memory_preview_set_after

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
