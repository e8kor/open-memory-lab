from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.grant_web_capacities_response_200_key import (
        GrantWebCapacitiesResponse200Key,
    )
    from ..models.whoami_grant import WhoamiGrant


T = TypeVar("T", bound="GrantWebCapacitiesResponse200")


@_attrs_define
class GrantWebCapacitiesResponse200:
    """
    Attributes:
        key (GrantWebCapacitiesResponse200Key):
        grants_added (list[WhoamiGrant]):
        journal_seq (int | None): The seq of the last grant.issued, or null when none was added.
    """

    key: GrantWebCapacitiesResponse200Key
    grants_added: list[WhoamiGrant]
    journal_seq: int | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        key = self.key.to_dict()

        grants_added = []
        for grants_added_item_data in self.grants_added:
            grants_added_item = grants_added_item_data.to_dict()
            grants_added.append(grants_added_item)

        journal_seq: int | None
        journal_seq = self.journal_seq

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "key": key,
                "grants_added": grants_added,
                "journal_seq": journal_seq,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.grant_web_capacities_response_200_key import (
            GrantWebCapacitiesResponse200Key,
        )
        from ..models.whoami_grant import WhoamiGrant

        d = dict(src_dict)
        key = GrantWebCapacitiesResponse200Key.from_dict(d.pop("key"))

        grants_added = []
        _grants_added = d.pop("grants_added")
        for grants_added_item_data in _grants_added:
            grants_added_item = WhoamiGrant.from_dict(grants_added_item_data)

            grants_added.append(grants_added_item)

        def _parse_journal_seq(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        journal_seq = _parse_journal_seq(d.pop("journal_seq"))

        grant_web_capacities_response_200 = cls(
            key=key,
            grants_added=grants_added,
            journal_seq=journal_seq,
        )

        grant_web_capacities_response_200.additional_properties = d
        return grant_web_capacities_response_200

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
