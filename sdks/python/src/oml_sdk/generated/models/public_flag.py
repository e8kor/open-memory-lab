from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="PublicFlag")


@_attrs_define
class PublicFlag:
    """Whether every reader, contributor and reviewer key of the tenant reads the node's approved items without a grant,
    and every default recall searches it (ADR-0098 d1, d3, d9). It covers the node alone. Since v0.8.6.

        Attributes:
            value (bool):
            since (None | str): When the act that set it took effect; null when not public.
            act_id (None | str): The admin act that set it; null when not public.
            sole_admin (bool): Whether that act took effect at once because the tenant has only ever had one admin person
                (ADR-0097 d10).
            pending_act_id (str | Unset): To an admin key on the workspace read only: a set that waits on a second admin
                person.
    """

    value: bool
    since: None | str
    act_id: None | str
    sole_admin: bool
    pending_act_id: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        value = self.value

        since: None | str
        since = self.since

        act_id: None | str
        act_id = self.act_id

        sole_admin = self.sole_admin

        pending_act_id = self.pending_act_id

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "value": value,
                "since": since,
                "act_id": act_id,
                "sole_admin": sole_admin,
            }
        )
        if pending_act_id is not UNSET:
            field_dict["pending_act_id"] = pending_act_id

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        value = d.pop("value")

        def _parse_since(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        since = _parse_since(d.pop("since"))

        def _parse_act_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        act_id = _parse_act_id(d.pop("act_id"))

        sole_admin = d.pop("sole_admin")

        pending_act_id = d.pop("pending_act_id", UNSET)

        public_flag = cls(
            value=value,
            since=since,
            act_id=act_id,
            sole_admin=sole_admin,
            pending_act_id=pending_act_id,
        )

        return public_flag
