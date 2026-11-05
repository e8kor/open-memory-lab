from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define

from ..models.recall_event_labels_item_label import RecallEventLabelsItemLabel
from ..models.recall_event_labels_item_source import RecallEventLabelsItemSource

T = TypeVar("T", bound="RecallEventLabelsItem")


@_attrs_define
class RecallEventLabelsItem:
    """
    Attributes:
        artifact_id (None | UUID): Null for requery and no_use, which label the whole recall.
        label (RecallEventLabelsItemLabel):
        source (RecallEventLabelsItemSource):
        at (str):
    """

    artifact_id: None | UUID
    label: RecallEventLabelsItemLabel
    source: RecallEventLabelsItemSource
    at: str

    def to_dict(self) -> dict[str, Any]:
        artifact_id: None | str
        if isinstance(self.artifact_id, UUID):
            artifact_id = str(self.artifact_id)
        else:
            artifact_id = self.artifact_id

        label = self.label.value

        source = self.source.value

        at = self.at

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "artifact_id": artifact_id,
                "label": label,
                "source": source,
                "at": at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_artifact_id(data: object) -> None | UUID:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                artifact_id_type_0 = UUID(data)

                return artifact_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | UUID, data)

        artifact_id = _parse_artifact_id(d.pop("artifact_id"))

        label = RecallEventLabelsItemLabel(d.pop("label"))

        source = RecallEventLabelsItemSource(d.pop("source"))

        at = d.pop("at")

        recall_event_labels_item = cls(
            artifact_id=artifact_id,
            label=label,
            source=source,
            at=at,
        )

        return recall_event_labels_item
