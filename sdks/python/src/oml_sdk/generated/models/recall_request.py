from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.recall_request_kinds_item import RecallRequestKindsItem
from ..models.recall_request_match import RecallRequestMatch
from ..models.recall_request_search import RecallRequestSearch
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.feedback import Feedback


T = TypeVar("T", bound="RecallRequest")


@_attrs_define
class RecallRequest:
    """The scope is workarea's subtree, else the key's default_workarea's (ADR-0039 d4). Unknown fields are refused
    (request.validation), and so are the designed vocabulary filter's terms and expand_terms, declared in openapi-
    designed.yaml (ADR-0039 d9); v0.3.0's include_bodies and include_own_held are request.validation naming include_text
    and include_unreviewed in details.renamed_to (ADR-0039 d7).

        Attributes:
            query (str):
            workarea (str | Unset): A workarea, by its node id, its name or its qualified name. Resolved server-side before
                any check that reads state; an unknown name answers exactly what an unknown id answers. A user key resolves a
                bare name in its own user root's namespace, plus universe and its root's name; every other key in the shared
                namespace (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is universe,
                is neither, and is refused (request.validation).
            k (int | Unset):  Default: 8.
            kinds (list[RecallRequestKindsItem] | Unset): Filters hits only; an empty array is refused (request.validation),
                so only an absent kinds means no filter. Deliberately excludes steering (never ranked, always first) and the
                vocabulary kinds (excluded from the shared recall indexes by default — ADR-0021 d9); where a vocabulary
                workarea's journalled policy opts them in, they are hits only of a recall that omits kinds.
            include_text (bool | Unset): Whether each served item carries its text (ADR-0039 d7). Default: True.
            include_unreviewed (bool | Unset): When true, the calling key's own held rows (kind memory or derived_fact,
                never steering or retirement versions) inside subtree(scope) ∩ reader effective access, admitted by the kinds
                filter when one is sent as promoted hits are, join the hits, ranked in the same order, with memory.state held;
                they count against k and max_tokens. Rules: docs/architecture/retrieval.md. Default: False.
            max_tokens (int | Unset): Recall budget in budget tokens (counter oml-tc/1), at most 2^53 - 1, the largest
                integer the request digest canonicalizes exactly under RFC 8785. Steering is never cut; hits are the longest
                rank prefix that fits. Refused with include_text false. Rules: docs/architecture/retrieval.md, Token budgets.
            search (RecallRequestSearch | Unset): The search pipeline, among those the deployment serves (search.enabled);
                omitted, the tenant's default, else the deployment's (ADR-0049 d4; bm25-postings, ADR-0052 d9; sparse and
                bm25-sparse, learned sparse weights alone or fused with bm25-postings, ADR-0052 d16; bm25-postings-time,
                bm25-postings fused with the memories whose valid time overlaps a date the query names, and bm25-postings-time-
                recent, which also moves each of the first 50 scores by at most 10% by recency, proposed ADR-0058 d7, d8). A
                name the deployment does not serve is request.validation with details.field /search and details.served.
            match (RecallRequestMatch | Unset): all keeps only rows holding every query word; when none does, the any-word
                hits are served and search.fallback is any_term (ADR-0049 d5). Default: RecallRequestMatch.ANY.
            feedback (Feedback | Unset): Which hits of this key's earlier recall answered its question and which did not
                (ADR-0054 d6). Checked before the read runs: at most 20 ids, each named once, a recall_id this key received in
                the last 24 hours, and only ids that recall served it; anything else is request.validation naming the field
                (/feedback/recall_id, /feedback/useful/0, ...) and the read is not run. A key's explicit labels past 1000 a UTC
                day answer limit.rate. A valid member changes nothing the read answers.
            include_expired (bool | Unset): true also serves memories whose valid_until has passed, which recall hides by
                default; get and the listings always serve them (proposed ADR-0058 d3). Default: False.
            now (str | Unset): YYYY, YYYY-MM, YYYY-MM-DD, or an RFC 3339 date-time with an offset and at most six fractional
                digits (proposed ADR-0058 d2).
    """

    query: str
    workarea: str | Unset = UNSET
    k: int | Unset = 8
    kinds: list[RecallRequestKindsItem] | Unset = UNSET
    include_text: bool | Unset = True
    include_unreviewed: bool | Unset = False
    max_tokens: int | Unset = UNSET
    search: RecallRequestSearch | Unset = UNSET
    match: RecallRequestMatch | Unset = RecallRequestMatch.ANY
    feedback: Feedback | Unset = UNSET
    include_expired: bool | Unset = False
    now: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        query = self.query

        workarea: str | Unset
        if isinstance(self.workarea, Unset):
            workarea = UNSET
        else:
            workarea = self.workarea

        k = self.k

        kinds: list[str] | Unset = UNSET
        if not isinstance(self.kinds, Unset):
            kinds = []
            for kinds_item_data in self.kinds:
                kinds_item = kinds_item_data.value
                kinds.append(kinds_item)

        include_text = self.include_text

        include_unreviewed = self.include_unreviewed

        max_tokens = self.max_tokens

        search: str | Unset = UNSET
        if not isinstance(self.search, Unset):
            search = self.search.value

        match: str | Unset = UNSET
        if not isinstance(self.match, Unset):
            match = self.match.value

        feedback: dict[str, Any] | Unset = UNSET
        if not isinstance(self.feedback, Unset):
            feedback = self.feedback.to_dict()

        include_expired = self.include_expired

        now = self.now

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "query": query,
            }
        )
        if workarea is not UNSET:
            field_dict["workarea"] = workarea
        if k is not UNSET:
            field_dict["k"] = k
        if kinds is not UNSET:
            field_dict["kinds"] = kinds
        if include_text is not UNSET:
            field_dict["include_text"] = include_text
        if include_unreviewed is not UNSET:
            field_dict["include_unreviewed"] = include_unreviewed
        if max_tokens is not UNSET:
            field_dict["max_tokens"] = max_tokens
        if search is not UNSET:
            field_dict["search"] = search
        if match is not UNSET:
            field_dict["match"] = match
        if feedback is not UNSET:
            field_dict["feedback"] = feedback
        if include_expired is not UNSET:
            field_dict["include_expired"] = include_expired
        if now is not UNSET:
            field_dict["now"] = now

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.feedback import Feedback

        d = dict(src_dict)
        query = d.pop("query")

        def _parse_workarea(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        workarea = _parse_workarea(d.pop("workarea", UNSET))

        k = d.pop("k", UNSET)

        _kinds = d.pop("kinds", UNSET)
        kinds: list[RecallRequestKindsItem] | Unset = UNSET
        if _kinds is not UNSET:
            kinds = []
            for kinds_item_data in _kinds:
                kinds_item = RecallRequestKindsItem(kinds_item_data)

                kinds.append(kinds_item)

        include_text = d.pop("include_text", UNSET)

        include_unreviewed = d.pop("include_unreviewed", UNSET)

        max_tokens = d.pop("max_tokens", UNSET)

        _search = d.pop("search", UNSET)
        search: RecallRequestSearch | Unset
        if isinstance(_search, Unset):
            search = UNSET
        else:
            search = RecallRequestSearch(_search)

        _match = d.pop("match", UNSET)
        match: RecallRequestMatch | Unset
        if isinstance(_match, Unset):
            match = UNSET
        else:
            match = RecallRequestMatch(_match)

        _feedback = d.pop("feedback", UNSET)
        feedback: Feedback | Unset
        if isinstance(_feedback, Unset):
            feedback = UNSET
        else:
            feedback = Feedback.from_dict(_feedback)

        include_expired = d.pop("include_expired", UNSET)

        now = d.pop("now", UNSET)

        recall_request = cls(
            query=query,
            workarea=workarea,
            k=k,
            kinds=kinds,
            include_text=include_text,
            include_unreviewed=include_unreviewed,
            max_tokens=max_tokens,
            search=search,
            match=match,
            feedback=feedback,
            include_expired=include_expired,
            now=now,
        )

        return recall_request
