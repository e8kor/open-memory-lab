from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.review_next_body_needs import ReviewNextBodyNeeds
from ..types import UNSET, Unset

T = TypeVar("T", bound="ReviewNextBody")


@_attrs_define
class ReviewNextBody:
    """The queue is served whole: the subtree filter workspace is a designed field (openapi-designed.yaml x-oml-designed-
    fields), refused with request.validation until it is built (ADR-0039 d9).

        Attributes:
            needs (ReviewNextBodyNeeds | Unset): person serves the person lane alone: steering, guardrail and skill versions
                and a successor of another kind that replaces one, per author key, the author holding the fewest first and each
                author's oldest first; a key that cannot approve them is served none, 204. reviewer serves the rows any reviewer
                may approve. Omitted, a person's reviewer key is served every row and any other key the rows it may approve
                (ADR-0074 d19; ADR-0080 d16). Since v0.8.0.
    """

    needs: ReviewNextBodyNeeds | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        needs: str | Unset = UNSET
        if not isinstance(self.needs, Unset):
            needs = self.needs.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if needs is not UNSET:
            field_dict["needs"] = needs

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        _needs = d.pop("needs", UNSET)
        needs: ReviewNextBodyNeeds | Unset
        if isinstance(_needs, Unset):
            needs = UNSET
        else:
            needs = ReviewNextBodyNeeds(_needs)

        review_next_body = cls(
            needs=needs,
        )

        review_next_body.additional_properties = d
        return review_next_body

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
