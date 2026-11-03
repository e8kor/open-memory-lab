from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.recall_request_kinds_item import RecallRequestKindsItem
from ..types import UNSET, Unset

T = TypeVar("T", bound="RecallRequest")


@_attrs_define
class RecallRequest:
    """Names no workarea: the default scope is the token workarea's subtree — the key's default_workarea, or the X-OML-
    Workarea override (ADR-0022 d4). Unknown fields are refused (request.validation), and so are the designed vocabulary
    filter's terms and expand_terms, declared in openapi-designed.yaml (ADR-0039 d9).

        Attributes:
            query (str):
            k (int | Unset):  Default: 8.
            kinds (list[RecallRequestKindsItem] | Unset): Filters hits only; an empty array is refused (request.validation),
                so only an absent kinds means no filter. Deliberately excludes steering (never ranked, always first) and the
                vocabulary kinds (excluded from the shared recall indexes by default — ADR-0021 d9); where a vocabulary
                workarea's journalled policy opts them in, they are hits only of a recall that omits kinds.
            include_bodies (bool | Unset):  Default: True.
            include_own_held (bool | Unset): When true, the calling key's own held rows (kind memory or derived_fact, never
                steering or retirement versions) inside subtree(scope) ∩ reader effective access, admitted by the kinds filter
                when one is sent as promoted hits are, join the hits, ranked in the same order, with memory.state held; they
                count against k and max_tokens. Rules: docs/architecture/retrieval.md. Default: False.
            max_tokens (int | Unset): Recall budget in budget tokens (counter oml-tc/1), at most 2^53 - 1, the largest
                integer the request digest canonicalizes exactly under RFC 8785. Steering is never cut; hits are the longest
                rank prefix that fits. Refused with include_bodies false. Rules: docs/architecture/retrieval.md, Token budgets.
    """

    query: str
    k: int | Unset = 8
    kinds: list[RecallRequestKindsItem] | Unset = UNSET
    include_bodies: bool | Unset = True
    include_own_held: bool | Unset = False
    max_tokens: int | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        query = self.query

        k = self.k

        kinds: list[str] | Unset = UNSET
        if not isinstance(self.kinds, Unset):
            kinds = []
            for kinds_item_data in self.kinds:
                kinds_item = kinds_item_data.value
                kinds.append(kinds_item)

        include_bodies = self.include_bodies

        include_own_held = self.include_own_held

        max_tokens = self.max_tokens

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "query": query,
            }
        )
        if k is not UNSET:
            field_dict["k"] = k
        if kinds is not UNSET:
            field_dict["kinds"] = kinds
        if include_bodies is not UNSET:
            field_dict["include_bodies"] = include_bodies
        if include_own_held is not UNSET:
            field_dict["include_own_held"] = include_own_held
        if max_tokens is not UNSET:
            field_dict["max_tokens"] = max_tokens

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        query = d.pop("query")

        k = d.pop("k", UNSET)

        _kinds = d.pop("kinds", UNSET)
        kinds: list[RecallRequestKindsItem] | Unset = UNSET
        if _kinds is not UNSET:
            kinds = []
            for kinds_item_data in _kinds:
                kinds_item = RecallRequestKindsItem(kinds_item_data)

                kinds.append(kinds_item)

        include_bodies = d.pop("include_bodies", UNSET)

        include_own_held = d.pop("include_own_held", UNSET)

        max_tokens = d.pop("max_tokens", UNSET)

        recall_request = cls(
            query=query,
            k=k,
            kinds=kinds,
            include_bodies=include_bodies,
            include_own_held=include_own_held,
            max_tokens=max_tokens,
        )

        return recall_request
