from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="ReviewQueueResponse200ItemsItem")


@_attrs_define
class ReviewQueueResponse200ItemsItem:
    """
    Attributes:
        workspace_id (UUID):
        path (str):
        held (int):
        held_instructions (int): The held steering, guardrail and other instruction-class versions among held; only a
            person's reviewer key approves one (ADR-0080 d16).
        person_reviewers (int): The live person's reviewer keys whose effective access confers reviewer on the
            workspace. A count, never a key. It overcounts: a key whose person also wrote the held instructions is counted,
            though outside that person's own root it cannot approve them (review.author_in_lineage, same_person; ADR-0082
            d7), so a non-zero count does not prove that someone can approve each one.
        oldest_at (datetime.datetime):
    """

    workspace_id: UUID
    path: str
    held: int
    held_instructions: int
    person_reviewers: int
    oldest_at: datetime.datetime
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        workspace_id = str(self.workspace_id)

        path = self.path

        held = self.held

        held_instructions = self.held_instructions

        person_reviewers = self.person_reviewers

        oldest_at = self.oldest_at.isoformat()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "workspace_id": workspace_id,
                "path": path,
                "held": held,
                "held_instructions": held_instructions,
                "person_reviewers": person_reviewers,
                "oldest_at": oldest_at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        workspace_id = UUID(d.pop("workspace_id"))

        path = d.pop("path")

        held = d.pop("held")

        held_instructions = d.pop("held_instructions")

        person_reviewers = d.pop("person_reviewers")

        oldest_at = datetime.datetime.fromisoformat(d.pop("oldest_at"))

        review_queue_response_200_items_item = cls(
            workspace_id=workspace_id,
            path=path,
            held=held,
            held_instructions=held_instructions,
            person_reviewers=person_reviewers,
            oldest_at=oldest_at,
        )

        review_queue_response_200_items_item.additional_properties = d
        return review_queue_response_200_items_item

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
