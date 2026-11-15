from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.workspace_offers_refused_item_op import WorkspaceOffersRefusedItemOp
from ..models.workspace_offers_refused_item_to import WorkspaceOffersRefusedItemTo

T = TypeVar("T", bound="WorkspaceOffersRefusedItem")


@_attrs_define
class WorkspaceOffersRefusedItem:
    """
    Attributes:
        op (WorkspaceOffersRefusedItemOp):
        to (WorkspaceOffersRefusedItemTo):
        code (str): The error code the act's verb would answer.
        message (str):
    """

    op: WorkspaceOffersRefusedItemOp
    to: WorkspaceOffersRefusedItemTo
    code: str
    message: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        op = self.op.value

        to = self.to.value

        code = self.code

        message = self.message

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "op": op,
                "to": to,
                "code": code,
                "message": message,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        op = WorkspaceOffersRefusedItemOp(d.pop("op"))

        to = WorkspaceOffersRefusedItemTo(d.pop("to"))

        code = d.pop("code")

        message = d.pop("message")

        workspace_offers_refused_item = cls(
            op=op,
            to=to,
            code=code,
            message=message,
        )

        workspace_offers_refused_item.additional_properties = d
        return workspace_offers_refused_item

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
