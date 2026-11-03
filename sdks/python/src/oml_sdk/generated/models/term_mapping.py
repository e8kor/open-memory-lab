from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.term_mapping_justification import TermMappingJustification
from ..models.term_mapping_predicate import TermMappingPredicate
from ..types import UNSET, Unset

T = TypeVar("T", bound="TermMapping")


@_attrs_define
class TermMapping:
    """SSSOM-shaped ratified claim; revocable by supersession, never a merge.

    Attributes:
        subject (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once minted (first
            promotion).
        object_ (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once minted (first
            promotion).
        predicate (TermMappingPredicate):
        justification (TermMappingJustification):
        ratified_at (datetime.datetime):
        note (str | Unset):
    """

    subject: str
    object_: str
    predicate: TermMappingPredicate
    justification: TermMappingJustification
    ratified_at: datetime.datetime
    note: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        subject = self.subject

        object_ = self.object_

        predicate = self.predicate.value

        justification = self.justification.value

        ratified_at = self.ratified_at.isoformat()

        note = self.note

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "subject": subject,
                "object": object_,
                "predicate": predicate,
                "justification": justification,
                "ratified_at": ratified_at,
            }
        )
        if note is not UNSET:
            field_dict["note"] = note

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        subject = d.pop("subject")

        object_ = d.pop("object")

        predicate = TermMappingPredicate(d.pop("predicate"))

        justification = TermMappingJustification(d.pop("justification"))

        ratified_at = datetime.datetime.fromisoformat(d.pop("ratified_at"))

        note = d.pop("note", UNSET)

        term_mapping = cls(
            subject=subject,
            object_=object_,
            predicate=predicate,
            justification=justification,
            ratified_at=ratified_at,
            note=note,
        )

        term_mapping.additional_properties = d
        return term_mapping

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
