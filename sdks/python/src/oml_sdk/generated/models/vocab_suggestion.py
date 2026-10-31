from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.vocab_suggestion_term_status import VocabSuggestionTermStatus

T = TypeVar("T", bound="VocabSuggestion")


@_attrs_define
class VocabSuggestion:
    """
    Attributes:
        curie (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once minted (first
            promotion).
        label (str):
        term_status (VocabSuggestionTermStatus):
    """

    curie: str
    label: str
    term_status: VocabSuggestionTermStatus
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        curie = self.curie

        label = self.label

        term_status = self.term_status.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "curie": curie,
                "label": label,
                "term_status": term_status,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        curie = d.pop("curie")

        label = d.pop("label")

        term_status = VocabSuggestionTermStatus(d.pop("term_status"))

        vocab_suggestion = cls(
            curie=curie,
            label=label,
            term_status=term_status,
        )

        vocab_suggestion.additional_properties = d
        return vocab_suggestion

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
