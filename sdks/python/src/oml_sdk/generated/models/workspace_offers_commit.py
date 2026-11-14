from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="WorkareaOffersCommit")


@_attrs_define
class WorkareaOffersCommit:
    """
    Attributes:
        offered (bool): Committing the in-progress root is offered now.
        held (int | None): The held rows that withhold the commit, or null.
    """

    offered: bool
    held: int | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        offered = self.offered

        held: int | None
        held = self.held

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "offered": offered,
                "held": held,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        offered = d.pop("offered")

        def _parse_held(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        held = _parse_held(d.pop("held"))

        workarea_offers_commit = cls(
            offered=offered,
            held=held,
        )

        workarea_offers_commit.additional_properties = d
        return workarea_offers_commit

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
