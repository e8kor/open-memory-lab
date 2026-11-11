from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.origin_kind import OriginKind
from ..models.recall_request_kinds_item import RecallRequestKindsItem
from ..models.recall_request_match import RecallRequestMatch
from ..models.recall_request_search import RecallRequestSearch
from ..models.recall_request_sources_type_0 import RecallRequestSourcesType0
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
            anchor_repo (str | Unset): A repository, scheme, credentials and .git dropped (github.com/e8kor/open-memory-
                lab); no segment is . or .. (ADR-0057 d3). As a filter, rows whose anchor names it; unanchored rows are left out
                (d4).
            anchor_path_prefix (str | Unset): A byte prefix of the anchor path, at most 1,024 UTF-8 bytes; rows whose anchor
                path starts with it, unanchored rows left out (ADR-0057 d4).
            origin_kinds (list[OriginKind] | Unset): Rows whose origin kind is one of these, combined with every other
                filter by AND; rows without a kind are left out (ADR-0063 d3).
            include_unreviewed (bool | Unset): When true, the calling key's own held rows (kind memory or derived_fact,
                never steering or retirement versions) inside subtree(scope) ∩ reader effective access, admitted by the kinds
                filter when one is sent as promoted hits are, join the hits, ranked in the same order, with memory.state held
                and unreviewed true; they count against k and max_tokens. Inside a workarea a person put in progress, every
                author's held memories on the served nodes join too, and absent means true there; absent means false everywhere
                else (ADR-0030 d3, d4). Rules: docs/architecture/retrieval.md.
            max_tokens (int | Unset): Recall budget in budget tokens (counter oml-tc/1), at most 2^53 - 1, the largest
                integer the request digest canonicalizes exactly under RFC 8785. Steering is never cut; hits are the longest
                rank prefix that fits. Refused with include_text false. Rules: docs/architecture/retrieval.md, Token budgets.
            search (RecallRequestSearch | Unset): The search pipeline, among those the deployment serves (search.enabled);
                omitted, the tenant's default, else the deployment's (ADR-0049 d4; bm25-postings, ADR-0052 d9; sparse and
                bm25-sparse, learned sparse weights alone or fused with bm25-postings, ADR-0052 d16; bm25-sparse-rrf, the same
                two lists fused by reciprocal rank, the opt-in baseline of ADR-0052 d13; bm25-postings-time, bm25-postings fused
                with the memories whose valid time overlaps a date the query names, ADR-0059 d7). A name the deployment does not
                serve is request.validation with details.field /search and details.served.
            match (RecallRequestMatch | Unset): all keeps only rows holding every query word; when none does, the any-word
                hits are served and search.fallback is any_term (ADR-0049 d5). Default: RecallRequestMatch.ANY.
            feedback (Feedback | Unset): Which hits of this key's earlier recall answered its question and which did not
                (ADR-0054 d6). Checked before the read runs: at most 20 ids, each named once, a recall_id this key received in
                the last 24 hours, and only ids that recall served it; anything else is request.validation naming the field
                (/feedback/recall_id, /feedback/useful/0, ...) and the read is not run. A key's explicit labels past 1000 a UTC
                day answer limit.rate. A valid member changes nothing the read answers.
            timings (bool | Unset): Omitted, false. true serves the answer's per-stage timings; refused with
                request.validation, details.field /timings, unless the deployment sets search.timings (ADR-0049 d14). Since
                v0.5.9.
            include_expired (bool | Unset): true also serves memories whose valid_until has passed, which recall hides by
                default; get and the listings always serve them (ADR-0059 d3). Default: False.
            now (str | Unset): YYYY, YYYY-MM, YYYY-MM-DD, or an RFC 3339 date-time with an offset and at most six fractional
                digits. A period is read in UTC: 2026-11-05 runs from 00:00 to 24:00 UTC (ADR-0059 d2).
            as_of (datetime.datetime | Unset): Recall memory as it was at this past instant, under today's grants: each
                claim's tip then, the caller's held rows recorded by then, and the steering in force then; scores are today's. A
                pipeline that reads postings is answered by bm25's scan over the scope at the instant, keyword-all by keyword-
                any with every word, and search.pipeline names the one asked. Only sources scope combines with it. An RFC 3339
                date-time; a date or an instant after the server's clock is request.validation naming /as_of. Digested with the
                request when sent (ADR-0059 d14, d15, d17, d18).
            sources (list[str] | RecallRequestSourcesType0 | Unset): Where to search (ADR-0072 d5). scope, the default, is
                the workarea's subtree as always. wide adds the committed and validated workareas the key already reads outside
                it that accept federated reads: all of them up to the tenant's max_sources, above it those holding the largest
                share of the query's matching rows. A list of 1 to max_sources distinct workarea names or ids searches exactly
                those, answered whole or refused. Every hit of wide or a list is scored with one set of statistics fixed before
                any selection, so no hit's score depends on which sources were selected (d7). Absent, the tenant's
                federation.default_sources. Refusals, in this order: an empty list, a name twice, more than max_sources names,
                or a non-string element is request.validation at /sources; wide or a list with federation off is
                request.validation at /sources; wide or a list on bm25-sparse is request.validation at /search; then each name
                in order: one the key cannot read and one that does not exist are the same access.not_found with details.field
                /sources/<i>, and a readable name that is not committed or validated, or lies outside the scope on a workarea
                declining federated reads, is request.validation at /sources/<i> with details.reason source_untrusted or
                source_declines_federation. Since v0.7.0.
    """

    query: str
    workarea: str | Unset = UNSET
    k: int | Unset = 8
    kinds: list[RecallRequestKindsItem] | Unset = UNSET
    include_text: bool | Unset = True
    anchor_repo: str | Unset = UNSET
    anchor_path_prefix: str | Unset = UNSET
    origin_kinds: list[OriginKind] | Unset = UNSET
    include_unreviewed: bool | Unset = UNSET
    max_tokens: int | Unset = UNSET
    search: RecallRequestSearch | Unset = UNSET
    match: RecallRequestMatch | Unset = RecallRequestMatch.ANY
    feedback: Feedback | Unset = UNSET
    timings: bool | Unset = UNSET
    include_expired: bool | Unset = False
    now: str | Unset = UNSET
    as_of: datetime.datetime | Unset = UNSET
    sources: list[str] | RecallRequestSourcesType0 | Unset = UNSET

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

        anchor_repo = self.anchor_repo

        anchor_path_prefix = self.anchor_path_prefix

        origin_kinds: list[str] | Unset = UNSET
        if not isinstance(self.origin_kinds, Unset):
            origin_kinds = []
            for componentsschemas_origin_kinds_item_data in self.origin_kinds:
                componentsschemas_origin_kinds_item = (
                    componentsschemas_origin_kinds_item_data.value
                )
                origin_kinds.append(componentsschemas_origin_kinds_item)

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

        timings = self.timings

        include_expired = self.include_expired

        now = self.now

        as_of: str | Unset = UNSET
        if not isinstance(self.as_of, Unset):
            as_of = self.as_of.isoformat()

        sources: list[str] | str | Unset
        if isinstance(self.sources, Unset):
            sources = UNSET
        elif isinstance(self.sources, RecallRequestSourcesType0):
            sources = self.sources.value
        else:
            sources = self.sources

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
        if anchor_repo is not UNSET:
            field_dict["anchor_repo"] = anchor_repo
        if anchor_path_prefix is not UNSET:
            field_dict["anchor_path_prefix"] = anchor_path_prefix
        if origin_kinds is not UNSET:
            field_dict["origin_kinds"] = origin_kinds
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
        if timings is not UNSET:
            field_dict["timings"] = timings
        if include_expired is not UNSET:
            field_dict["include_expired"] = include_expired
        if now is not UNSET:
            field_dict["now"] = now
        if as_of is not UNSET:
            field_dict["as_of"] = as_of
        if sources is not UNSET:
            field_dict["sources"] = sources

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

        anchor_repo = d.pop("anchor_repo", UNSET)

        anchor_path_prefix = d.pop("anchor_path_prefix", UNSET)

        _origin_kinds = d.pop("origin_kinds", UNSET)
        origin_kinds: list[OriginKind] | Unset = UNSET
        if _origin_kinds is not UNSET:
            origin_kinds = []
            for componentsschemas_origin_kinds_item_data in _origin_kinds:
                componentsschemas_origin_kinds_item = OriginKind(
                    componentsschemas_origin_kinds_item_data
                )

                origin_kinds.append(componentsschemas_origin_kinds_item)

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

        timings = d.pop("timings", UNSET)

        include_expired = d.pop("include_expired", UNSET)

        now = d.pop("now", UNSET)

        _as_of = d.pop("as_of", UNSET)
        as_of: datetime.datetime | Unset
        if isinstance(_as_of, Unset):
            as_of = UNSET
        else:
            as_of = datetime.datetime.fromisoformat(_as_of)

        def _parse_sources(
            data: object,
        ) -> list[str] | RecallRequestSourcesType0 | Unset:
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                sources_type_0 = RecallRequestSourcesType0(data)

                return sources_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, list):
                raise TypeError()
            sources_type_1 = cast(list[str], data)

            return sources_type_1

        sources = _parse_sources(d.pop("sources", UNSET))

        recall_request = cls(
            query=query,
            workarea=workarea,
            k=k,
            kinds=kinds,
            include_text=include_text,
            anchor_repo=anchor_repo,
            anchor_path_prefix=anchor_path_prefix,
            origin_kinds=origin_kinds,
            include_unreviewed=include_unreviewed,
            max_tokens=max_tokens,
            search=search,
            match=match,
            feedback=feedback,
            timings=timings,
            include_expired=include_expired,
            now=now,
            as_of=as_of,
            sources=sources,
        )

        return recall_request
