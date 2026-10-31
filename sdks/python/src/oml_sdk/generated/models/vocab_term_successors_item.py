from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.vocab_term_successors_item_relation import VocabTermSuccessorsItemRelation

T = TypeVar("T", bound="VocabTermSuccessorsItem")


@_attrs_define
class VocabTermSuccessorsItem:
    """
    Attributes:
        curie (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once minted (first
            promotion).
        relation (VocabTermSuccessorsItemRelation): replaced_by is safe for automated rewiring; consider requires
            judgment.
    """

    curie: str
    relation: VocabTermSuccessorsItemRelation
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        curie = self.curie

        relation = self.relation.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "curie": curie,
                "relation": relation,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        curie = d.pop("curie")

        relation = VocabTermSuccessorsItemRelation(d.pop("relation"))

        vocab_term_successors_item = cls(
            curie=curie,
            relation=relation,
        )

        vocab_term_successors_item.additional_properties = d
        return vocab_term_successors_item

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
