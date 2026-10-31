from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.memory import Memory
    from ..models.served_digests import ServedDigests


T = TypeVar("T", bound="Serve")


@_attrs_define
class Serve:
    """
    Attributes:
        serve_id (str): Bound server-side to (reviewer key, artifact version, attachment digest set); consumed by its
            first verdict.
        expires_at (datetime.datetime):
        item (Memory):
        served_digests (ServedDigests):
        replaces (Memory | Unset):
    """

    serve_id: str
    expires_at: datetime.datetime
    item: Memory
    served_digests: ServedDigests
    replaces: Memory | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        serve_id = self.serve_id

        expires_at = self.expires_at.isoformat()

        item = self.item.to_dict()

        served_digests = self.served_digests.to_dict()

        replaces: dict[str, Any] | Unset = UNSET
        if not isinstance(self.replaces, Unset):
            replaces = self.replaces.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "serve_id": serve_id,
                "expires_at": expires_at,
                "item": item,
                "served_digests": served_digests,
            }
        )
        if replaces is not UNSET:
            field_dict["replaces"] = replaces

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.memory import Memory
        from ..models.served_digests import ServedDigests

        d = dict(src_dict)
        serve_id = d.pop("serve_id")

        expires_at = datetime.datetime.fromisoformat(d.pop("expires_at"))

        item = Memory.from_dict(d.pop("item"))

        served_digests = ServedDigests.from_dict(d.pop("served_digests"))

        _replaces = d.pop("replaces", UNSET)
        replaces: Memory | Unset
        if isinstance(_replaces, Unset):
            replaces = UNSET
        else:
            replaces = Memory.from_dict(_replaces)

        serve = cls(
            serve_id=serve_id,
            expires_at=expires_at,
            item=item,
            served_digests=served_digests,
            replaces=replaces,
        )

        serve.additional_properties = d
        return serve

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
