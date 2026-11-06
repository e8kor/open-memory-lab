from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.similar_item_kind import SimilarItemKind
from ..models.similar_item_relation import SimilarItemRelation
from ..models.similar_item_state import SimilarItemState

T = TypeVar("T", bound="SimilarItem")


@_attrs_define
class SimilarItem:
    """A row the key may read that holds the same words in the same order (same_text) or a word set at Jaccard 0.9 or more
    (near_copy), found by integer keys and verified exactly, with no model (ADR-0050 d1, d17). Ordered same_text first,
    then by jaccard, then by artifact_id. To replace it, write with replaces set to artifact_id.

        Attributes:
            artifact_id (str):
            claim_key (str):
            version (int):
            kind (SimilarItemKind):
            workarea_id (str):
            state (SimilarItemState): held only for the calling key's own rows.
            relation (SimilarItemRelation):
            jaccard (float): The two word sets' Jaccard similarity, rounded down to hundredths; 1 for same_text.
            own (bool): The calling key wrote it.
    """

    artifact_id: str
    claim_key: str
    version: int
    kind: SimilarItemKind
    workarea_id: str
    state: SimilarItemState
    relation: SimilarItemRelation
    jaccard: float
    own: bool

    def to_dict(self) -> dict[str, Any]:
        artifact_id = self.artifact_id

        claim_key = self.claim_key

        version = self.version

        kind = self.kind.value

        workarea_id = self.workarea_id

        state = self.state.value

        relation = self.relation.value

        jaccard = self.jaccard

        own = self.own

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "artifact_id": artifact_id,
                "claim_key": claim_key,
                "version": version,
                "kind": kind,
                "workarea_id": workarea_id,
                "state": state,
                "relation": relation,
                "jaccard": jaccard,
                "own": own,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        artifact_id = d.pop("artifact_id")

        claim_key = d.pop("claim_key")

        version = d.pop("version")

        kind = SimilarItemKind(d.pop("kind"))

        workarea_id = d.pop("workarea_id")

        state = SimilarItemState(d.pop("state"))

        relation = SimilarItemRelation(d.pop("relation"))

        jaccard = d.pop("jaccard")

        own = d.pop("own")

        similar_item = cls(
            artifact_id=artifact_id,
            claim_key=claim_key,
            version=version,
            kind=kind,
            workarea_id=workarea_id,
            state=state,
            relation=relation,
            jaccard=jaccard,
            own=own,
        )

        return similar_item
