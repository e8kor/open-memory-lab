from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="PostingsHealth")


@_attrs_define
class PostingsHealth:
    """The tenant's integer postings, as doctor reads them (ADR-0052 d12). Present on GET /v2/search where the deployment
    serves bm25-postings; absent from the answer to POST /v2/search/default.

        Attributes:
            partition (bool): The tenant's postings partition exists.
            unfilled (int): Memory versions that should have postings and have none: the fill incomplete.
            served_behind (int): Served memories with no served posting, which bm25-postings cannot find.
            workarea_drift (int): Per-workarea counts that disagree with the served memories.
            term_drift (int): Sampled word counts that disagree with the served postings, or are missing.
            terms_checked (int): How many word counts were sampled.
    """

    partition: bool
    unfilled: int
    served_behind: int
    workarea_drift: int
    term_drift: int
    terms_checked: int

    def to_dict(self) -> dict[str, Any]:
        partition = self.partition

        unfilled = self.unfilled

        served_behind = self.served_behind

        workarea_drift = self.workarea_drift

        term_drift = self.term_drift

        terms_checked = self.terms_checked

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "partition": partition,
                "unfilled": unfilled,
                "served_behind": served_behind,
                "workarea_drift": workarea_drift,
                "term_drift": term_drift,
                "terms_checked": terms_checked,
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

        postings_health = cls(
            partition=partition,
            unfilled=unfilled,
            served_behind=served_behind,
            workarea_drift=workarea_drift,
            term_drift=term_drift,
            terms_checked=terms_checked,
        )

        return postings_health
