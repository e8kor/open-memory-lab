from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.admin_search_memories_body_origin_kind import (
    AdminSearchMemoriesBodyOriginKind,
)
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
        origin_kind (AdminSearchMemoriesBodyOriginKind | Unset): One origin.kind (ADR-0045 d22, d24; ADR-0070 d5). Since
            v0.6.4.
        author (str | Unset): The authoring key of the tenant, by name or id (ADR-0045 d24; ADR-0070 d5). Since v0.6.4.
        limit (int | Unset):
        cursor (str | Unset):
    """

    query: str | Unset = UNSET
    workarea: str | Unset = UNSET
    state: AdminSearchMemoriesBodyState | Unset = UNSET
    origin_kind: AdminSearchMemoriesBodyOriginKind | Unset = UNSET
    author: str | Unset = UNSET
    limit: int | Unset = UNSET
    cursor: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        query = self.query

        workarea = self.workarea

        state: str | Unset = UNSET
        if not isinstance(self.state, Unset):
            state = self.state.value

        origin_kind: str | Unset = UNSET
        if not isinstance(self.origin_kind, Unset):
            origin_kind = self.origin_kind.value

        author = self.author

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
        if origin_kind is not UNSET:
            field_dict["origin_kind"] = origin_kind
        if author is not UNSET:
            field_dict["author"] = author
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

        _origin_kind = d.pop("origin_kind", UNSET)
        origin_kind: AdminSearchMemoriesBodyOriginKind | Unset
        if isinstance(_origin_kind, Unset):
            origin_kind = UNSET
        else:
            origin_kind = AdminSearchMemoriesBodyOriginKind(_origin_kind)

        author = d.pop("author", UNSET)

        limit = d.pop("limit", UNSET)

        cursor = d.pop("cursor", UNSET)

        admin_search_memories_body = cls(
            query=query,
            workarea=workarea,
            state=state,
            origin_kind=origin_kind,
            author=author,
            limit=limit,
            cursor=cursor,
        )

        return admin_search_memories_body
