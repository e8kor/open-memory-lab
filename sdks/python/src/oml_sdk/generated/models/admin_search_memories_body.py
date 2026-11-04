from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.admin_search_memories_body_state import AdminSearchMemoriesBodyState
from ..types import UNSET, Unset

T = TypeVar("T", bound="AdminSearchMemoriesBody")


@_attrs_define
class AdminSearchMemoriesBody:
    """
    Attributes:
        query (str | Unset):
        workarea (str | Unset):
        state (AdminSearchMemoriesBodyState | Unset):
        limit (int | Unset):
        cursor (str | Unset):
    """

    query: str | Unset = UNSET
    workarea: str | Unset = UNSET
    state: AdminSearchMemoriesBodyState | Unset = UNSET
    limit: int | Unset = UNSET
    cursor: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        query = self.query

        workarea = self.workarea

        state: str | Unset = UNSET
        if not isinstance(self.state, Unset):
            state = self.state.value

        limit = self.limit

        cursor = self.cursor

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if query is not UNSET:
            field_dict["query"] = query
        if workarea is not UNSET:
            field_dict["workarea"] = workarea
        if state is not UNSET:
            field_dict["state"] = state
        if limit is not UNSET:
            field_dict["limit"] = limit
        if cursor is not UNSET:
            field_dict["cursor"] = cursor

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        query = d.pop("query", UNSET)

        workarea = d.pop("workarea", UNSET)

        _state = d.pop("state", UNSET)
        state: AdminSearchMemoriesBodyState | Unset
        if isinstance(_state, Unset):
            state = UNSET
        else:
            state = AdminSearchMemoriesBodyState(_state)

        limit = d.pop("limit", UNSET)

        cursor = d.pop("cursor", UNSET)

        admin_search_memories_body = cls(
            query=query,
            workarea=workarea,
            state=state,
            limit=limit,
            cursor=cursor,
        )

        return admin_search_memories_body
