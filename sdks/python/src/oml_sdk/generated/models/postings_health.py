from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="PostingsHealth")


@_attrs_define
class PostingsHealth:
    """The tenant's integer postings, as doctor reads them (ADR-0052 d12). Present on GET /v2/search where the deployment
    serves bm25-postings or a sparse pipeline; absent from the answer to POST /v2/search/default.

        Attributes:
            partition (bool): The tenant's postings partition exists.
            unfilled (int): Memory versions that should have postings and have none: the fill incomplete.
            served_behind (int): Served memories with no served posting, which bm25-postings cannot find.
            workarea_drift (int): Per-workarea counts that disagree with the served memories.
            term_drift (int): Sampled word counts that disagree with the served postings, or are missing.
            terms_checked (int): How many word counts were sampled.
            sparse_backlog (int): Memory versions waiting for the learned sparse encoder, parked ones aside, absent from
                sparse lists until it writes their postings (ADR-0052 d15).
            sparse_failing (int): Of them, those whose last encode failed; each is retried after at most an hour.
            sparse_parked (int): Memory versions parked after five failed encodes: kept with their reason, never retried
                until written again (ADR-0052 d15).
            sparse_enabled (bool): Whether the tenant's learned sparse queue is on: only a lead serving a sparse pipeline
                turns it on, and nothing is queued while it is off.
    """

    partition: bool
    unfilled: int
    served_behind: int
    workarea_drift: int
    term_drift: int
    terms_checked: int
    sparse_backlog: int
    sparse_failing: int
    sparse_parked: int
    sparse_enabled: bool

    def to_dict(self) -> dict[str, Any]:
        partition = self.partition

        unfilled = self.unfilled

        served_behind = self.served_behind

        workarea_drift = self.workarea_drift

        term_drift = self.term_drift

        terms_checked = self.terms_checked

        sparse_backlog = self.sparse_backlog

        sparse_failing = self.sparse_failing

        sparse_parked = self.sparse_parked

        sparse_enabled = self.sparse_enabled

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "partition": partition,
                "unfilled": unfilled,
                "served_behind": served_behind,
                "workarea_drift": workarea_drift,
                "term_drift": term_drift,
                "terms_checked": terms_checked,
                "sparse_backlog": sparse_backlog,
                "sparse_failing": sparse_failing,
                "sparse_parked": sparse_parked,
                "sparse_enabled": sparse_enabled,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        partition = d.pop("partition")

        unfilled = d.pop("unfilled")

        served_behind = d.pop("served_behind")

        workarea_drift = d.pop("workarea_drift")

        term_drift = d.pop("term_drift")

        terms_checked = d.pop("terms_checked")

        sparse_backlog = d.pop("sparse_backlog")

        sparse_failing = d.pop("sparse_failing")

        sparse_parked = d.pop("sparse_parked")

        sparse_enabled = d.pop("sparse_enabled")

        postings_health = cls(
            partition=partition,
            unfilled=unfilled,
            served_behind=served_behind,
            workarea_drift=workarea_drift,
            term_drift=term_drift,
            terms_checked=terms_checked,
            sparse_backlog=sparse_backlog,
            sparse_failing=sparse_failing,
            sparse_parked=sparse_parked,
            sparse_enabled=sparse_enabled,
        )

        return postings_health
