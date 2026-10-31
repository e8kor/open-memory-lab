from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.vocab_term_definition_state import VocabTermDefinitionState
from ..models.vocab_term_term_status import VocabTermTermStatus
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.vocab_term_successors_item import VocabTermSuccessorsItem


T = TypeVar("T", bound="VocabTerm")


@_attrs_define
class VocabTerm:
    """
    Attributes:
        curie (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once minted (first
            promotion).
        workarea_id (str): The owning vocabulary workarea.
        version (int): The served tip version of the term claim.
        term_status (VocabTermTermStatus): Structural payload field — survives shredding
        definition_state (VocabTermDefinitionState | Unset): The CURIE is permanent; the definition body is erasable
            (410 lifecycle.erased on fetch).
        successors (list[VocabTermSuccessorsItem] | Unset): Ratified successor edges declared with the deprecation
            version; empty for an active term.
        releases (list[str] | Unset): vocab_release ids whose pinned term set includes this term.
    """

    curie: str
    workarea_id: str
    version: int
    term_status: VocabTermTermStatus
    definition_state: VocabTermDefinitionState | Unset = UNSET
    successors: list[VocabTermSuccessorsItem] | Unset = UNSET
    releases: list[str] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        curie = self.curie

        workarea_id = self.workarea_id

        version = self.version

        term_status = self.term_status.value

        definition_state: str | Unset = UNSET
        if not isinstance(self.definition_state, Unset):
            definition_state = self.definition_state.value

        successors: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.successors, Unset):
            successors = []
            for successors_item_data in self.successors:
                successors_item = successors_item_data.to_dict()
                successors.append(successors_item)

        releases: list[str] | Unset = UNSET
        if not isinstance(self.releases, Unset):
            releases = self.releases

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "curie": curie,
                "workarea_id": workarea_id,
                "version": version,
                "term_status": term_status,
            }
        )
        if definition_state is not UNSET:
            field_dict["definition_state"] = definition_state
        if successors is not UNSET:
            field_dict["successors"] = successors
        if releases is not UNSET:
            field_dict["releases"] = releases

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.vocab_term_successors_item import VocabTermSuccessorsItem

        d = dict(src_dict)
        curie = d.pop("curie")

        workarea_id = d.pop("workarea_id")

        version = d.pop("version")

        term_status = VocabTermTermStatus(d.pop("term_status"))

        _definition_state = d.pop("definition_state", UNSET)
        definition_state: VocabTermDefinitionState | Unset
        if isinstance(_definition_state, Unset):
            definition_state = UNSET
        else:
            definition_state = VocabTermDefinitionState(_definition_state)

        _successors = d.pop("successors", UNSET)
        successors: list[VocabTermSuccessorsItem] | Unset = UNSET
        if _successors is not UNSET:
            successors = []
            for successors_item_data in _successors:
                successors_item = VocabTermSuccessorsItem.from_dict(
                    successors_item_data
                )

                successors.append(successors_item)

        releases = cast(list[str], d.pop("releases", UNSET))

        vocab_term = cls(
            curie=curie,
            workarea_id=workarea_id,
            version=version,
            term_status=term_status,
            definition_state=definition_state,
            successors=successors,
            releases=releases,
        )

        vocab_term.additional_properties = d
        return vocab_term

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
