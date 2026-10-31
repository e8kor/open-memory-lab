from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.supersede_request_mapping_justification import (
    SupersedeRequestMappingJustification,
)
from ..models.supersede_request_mapping_predicate import (
    SupersedeRequestMappingPredicate,
)
from ..types import UNSET, Unset

T = TypeVar("T", bound="SupersedeRequestMapping")


@_attrs_define
class SupersedeRequestMapping:
    """Successor of kind term_mapping only — required then, as on SubmitRequest. Structural payload fields (ADR-0021 d6).

    Attributes:
        subject (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once minted (first
            promotion).
        object_ (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once minted (first
            promotion).
        predicate (SupersedeRequestMappingPredicate):
        justification (SupersedeRequestMappingJustification):
        note (str | Unset):
    """

    subject: str
    object_: str
    predicate: SupersedeRequestMappingPredicate
    justification: SupersedeRequestMappingJustification
    note: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        subject = self.subject

        object_ = self.object_

        predicate = self.predicate.value

        justification = self.justification.value

        note = self.note

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "subject": subject,
                "object": object_,
                "predicate": predicate,
                "justification": justification,
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

        predicate = SupersedeRequestMappingPredicate(d.pop("predicate"))

        justification = SupersedeRequestMappingJustification(d.pop("justification"))

        note = d.pop("note", UNSET)

        supersede_request_mapping = cls(
            subject=subject,
            object_=object_,
            predicate=predicate,
            justification=justification,
            note=note,
        )

        return supersede_request_mapping
