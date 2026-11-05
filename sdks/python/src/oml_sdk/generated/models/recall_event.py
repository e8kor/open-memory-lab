from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define

from ..models.recall_event_access import RecallEventAccess

if TYPE_CHECKING:
    from ..models.recall_event_hits_item import RecallEventHitsItem
    from ..models.recall_event_labels_item import RecallEventLabelsItem


T = TypeVar("T", bound="RecallEvent")


@_attrs_define
class RecallEvent:
    """One recall as recorded (ADR-0054 d2).

    Attributes:
        recall_id (UUID):
        created_at (str):
        key_id (UUID):
        principal (str):
        workarea_id (UUID): The workarea the recall read.
        pipeline (str):
        access (RecallEventAccess):
        query_digest (str): SHA-256 of plaintext unless a field says ciphertext.
        query_text (None | str): Present only where the tenant kept text when the recall ran.
        hits (list[RecallEventHitsItem]):
        labels (list[RecallEventLabelsItem]):
    """

    recall_id: UUID
    created_at: str
    key_id: UUID
    principal: str
    workarea_id: UUID
    pipeline: str
    access: RecallEventAccess
    query_digest: str
    query_text: None | str
    hits: list[RecallEventHitsItem]
    labels: list[RecallEventLabelsItem]

    def to_dict(self) -> dict[str, Any]:
        recall_id = str(self.recall_id)

        created_at = self.created_at

        key_id = str(self.key_id)

        principal = self.principal

        workarea_id = str(self.workarea_id)

        pipeline = self.pipeline

        access = self.access.value

        query_digest = self.query_digest

        query_text: None | str
        query_text = self.query_text

        hits = []
        for hits_item_data in self.hits:
            hits_item = hits_item_data.to_dict()
            hits.append(hits_item)

        labels = []
        for labels_item_data in self.labels:
            labels_item = labels_item_data.to_dict()
            labels.append(labels_item)

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "recall_id": recall_id,
                "created_at": created_at,
                "key_id": key_id,
                "principal": principal,
                "workarea_id": workarea_id,
                "pipeline": pipeline,
                "access": access,
                "query_digest": query_digest,
                "query_text": query_text,
                "hits": hits,
                "labels": labels,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.recall_event_hits_item import RecallEventHitsItem
        from ..models.recall_event_labels_item import RecallEventLabelsItem

        d = dict(src_dict)
        recall_id = UUID(d.pop("recall_id"))

        created_at = d.pop("created_at")

        key_id = UUID(d.pop("key_id"))

        principal = d.pop("principal")

        workarea_id = UUID(d.pop("workarea_id"))

        pipeline = d.pop("pipeline")

        access = RecallEventAccess(d.pop("access"))

        query_digest = d.pop("query_digest")

        def _parse_query_text(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        query_text = _parse_query_text(d.pop("query_text"))

        hits = []
        _hits = d.pop("hits")
        for hits_item_data in _hits:
            hits_item = RecallEventHitsItem.from_dict(hits_item_data)

            hits.append(hits_item)

        labels = []
        _labels = d.pop("labels")
        for labels_item_data in _labels:
            labels_item = RecallEventLabelsItem.from_dict(labels_item_data)

            labels.append(labels_item)

        recall_event = cls(
            recall_id=recall_id,
            created_at=created_at,
            key_id=key_id,
            principal=principal,
            workarea_id=workarea_id,
            pipeline=pipeline,
            access=access,
            query_digest=query_digest,
            query_text=query_text,
            hits=hits,
            labels=labels,
        )

        return recall_event
