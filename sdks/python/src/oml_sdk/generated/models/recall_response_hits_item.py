from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.recall_response_hits_item_via import RecallResponseHitsItemVia
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.memory import Memory
    from ..models.recall_matched import RecallMatched
    from ..models.recall_passage import RecallPassage


T = TypeVar("T", bound="RecallResponseHitsItem")


@_attrs_define
class RecallResponseHitsItem:
    """
    Attributes:
        memory (Memory):
        score (float): The pipeline's score, comparable within one answer only; ties break by artifact_id ascending.
        matched (RecallMatched): Why a hit matched (ADR-0049 d6).
        via (RecallResponseHitsItemVia | Unset): Under sources wide or a list, and under scope where a public workspace
            was searched: scope when the hit's workspace is the recall's scope or under it, federated when it is another
            workspace the key reads, public when it is a public workspace outside the scope (ADR-0072 d8; ADR-0098 d3);
            memory.workspace_path names it.
        passage (RecallPassage | Unset): A document's hit only: the passage it serves (ADR-0081 d3, d4). A document is
            ranked by its passages and contributes one hit, its best passage by score, then ordinal; the hit's memory.text
            is that passage's lines byte for byte and memory.tokens its budget, so k counts claims and max_tokens counts the
            passage.
        more_passages (int | Unset): A document's hit only: how many of the document's other passages were in the fused
            list before the k cut, from any retriever. A lower bound, bounded by each list's depth, never an exact count of
            the passages holding a query word (ADR-0081 d4, the build's reading R3).
        uri (str | Unset): A document's hit only: the passage's resource,
            oml://workspace/<workspace>/doc/<path>?passage=<fragment>, which MCP resources/read serves (ADR-0081 d4, d6).
    """

    memory: Memory
    score: float
    matched: RecallMatched
    via: RecallResponseHitsItemVia | Unset = UNSET
    passage: RecallPassage | Unset = UNSET
    more_passages: int | Unset = UNSET
    uri: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        memory = self.memory.to_dict()

        score = self.score

        matched = self.matched.to_dict()

        via: str | Unset = UNSET
        if not isinstance(self.via, Unset):
            via = self.via.value

        passage: dict[str, Any] | Unset = UNSET
        if not isinstance(self.passage, Unset):
            passage = self.passage.to_dict()

        more_passages = self.more_passages

        uri = self.uri

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "memory": memory,
                "score": score,
                "matched": matched,
            }
        )
        if via is not UNSET:
            field_dict["via"] = via
        if passage is not UNSET:
            field_dict["passage"] = passage
        if more_passages is not UNSET:
            field_dict["more_passages"] = more_passages
        if uri is not UNSET:
            field_dict["uri"] = uri

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.memory import Memory
        from ..models.recall_matched import RecallMatched
        from ..models.recall_passage import RecallPassage

        d = dict(src_dict)
        memory = Memory.from_dict(d.pop("memory"))

        score = d.pop("score")

        matched = RecallMatched.from_dict(d.pop("matched"))

        _via = d.pop("via", UNSET)
        via: RecallResponseHitsItemVia | Unset
        if isinstance(_via, Unset):
            via = UNSET
        else:
            via = RecallResponseHitsItemVia(_via)

        _passage = d.pop("passage", UNSET)
        passage: RecallPassage | Unset
        if isinstance(_passage, Unset):
            passage = UNSET
        else:
            passage = RecallPassage.from_dict(_passage)

        more_passages = d.pop("more_passages", UNSET)

        uri = d.pop("uri", UNSET)

        recall_response_hits_item = cls(
            memory=memory,
            score=score,
            matched=matched,
            via=via,
            passage=passage,
            more_passages=more_passages,
            uri=uri,
        )

        recall_response_hits_item.additional_properties = d
        return recall_response_hits_item

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
