from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.supersede_request_successors_item_relation import (
    SupersedeRequestSuccessorsItemRelation,
)

T = TypeVar("T", bound="SupersedeRequestSuccessorsItem")


@_attrs_define
class SupersedeRequestSuccessorsItem:
    """
    Attributes:
        curie (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once minted (first
            promotion).
        relation (SupersedeRequestSuccessorsItemRelation):
    """

    curie: str
    relation: SupersedeRequestSuccessorsItemRelation

    def to_dict(self) -> dict[str, Any]:
        curie = self.curie

        relation = self.relation.value

        field_dict: dict[str, Any] = {}

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

        relation = SupersedeRequestSuccessorsItemRelation(d.pop("relation"))

        supersede_request_successors_item = cls(
            curie=curie,
            relation=relation,
        )

        return supersede_request_successors_item
