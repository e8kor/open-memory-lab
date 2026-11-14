from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.workarea_offers_transitions_item_op import WorkareaOffersTransitionsItemOp
from ..types import UNSET, Unset

T = TypeVar("T", bound="WorkareaOffersTransitionsItem")


@_attrs_define
class WorkareaOffersTransitionsItem:
    """
    Attributes:
        op (WorkareaOffersTransitionsItemOp):
        to (str):
        transition_id (UUID | Unset): The live proposal a ratify names.
        from_ (str | Unset):
    """

    op: WorkareaOffersTransitionsItemOp
    to: str
    transition_id: UUID | Unset = UNSET
    from_: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        op = self.op.value

        to = self.to

        transition_id: str | Unset = UNSET
        if not isinstance(self.transition_id, Unset):
            transition_id = str(self.transition_id)

        from_ = self.from_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "op": op,
                "to": to,
            }
        )
        if transition_id is not UNSET:
            field_dict["transition_id"] = transition_id
        if from_ is not UNSET:
            field_dict["from"] = from_

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        op = WorkareaOffersTransitionsItemOp(d.pop("op"))

        to = d.pop("to")

        _transition_id = d.pop("transition_id", UNSET)
        transition_id: UUID | Unset
        if isinstance(_transition_id, Unset):
            transition_id = UNSET
        else:
            transition_id = UUID(_transition_id)

        from_ = d.pop("from", UNSET)

        workarea_offers_transitions_item = cls(
            op=op,
            to=to,
            transition_id=transition_id,
            from_=from_,
        )

        workarea_offers_transitions_item.additional_properties = d
        return workarea_offers_transitions_item

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
