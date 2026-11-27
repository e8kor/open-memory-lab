from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="TodoCountsResponse200")


@_attrs_define
class TodoCountsResponse200:
    """
    Attributes:
        review (int): The To review group's length: the held rows the person's reviewer key may judge.
        briefs (int): The Brief proposals group's length: the pending brief edges GET /v2/review/briefs would list the
            person's reviewer key, at workspace and below or everywhere, never one it or its principal proposed; counted up
            to 1,000 and sharing truncated (ADR-0096 d7). Since v0.8.5.
        notices (int): The Notices group's length: the person's notices not dismissed, as GET /v2/web/notices lists
            them, whatever the workspace (ADR-0097 d20). Since v0.8.6.
        truncated (bool): Whether a count passed 1,000 and answers 1,000, or stopped after reading 2,000 held rows and
            answers what it counted.
    """

    review: int
    briefs: int
    notices: int
    truncated: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        review = self.review

        briefs = self.briefs

        notices = self.notices

        truncated = self.truncated

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "review": review,
                "briefs": briefs,
                "notices": notices,
                "truncated": truncated,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        review = d.pop("review")

        briefs = d.pop("briefs")

        notices = d.pop("notices")

        truncated = d.pop("truncated")

        todo_counts_response_200 = cls(
            review=review,
            briefs=briefs,
            notices=notices,
            truncated=truncated,
        )

        todo_counts_response_200.additional_properties = d
        return todo_counts_response_200

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
