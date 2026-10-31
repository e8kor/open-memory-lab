from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.grant_request_role import GrantRequestRole
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.grant_request_deny_item import GrantRequestDenyItem


T = TypeVar("T", bound="GrantRequest")


@_attrs_define
class GrantRequest:
    """
    Attributes:
        workarea_node_id (str): Node id, never a path.
        role (GrantRequestRole): A grant role, admissible under the key's role (the admissibility table in
            docs/architecture/roles-and-keys.md); never admin or operator.
        deny (list[GrantRequestDenyItem] | Unset): Per-grant masks; each masks a subtree within this grant only and must
            lie inside it (request.validation otherwise).
    """

    workarea_node_id: str
    role: GrantRequestRole
    deny: list[GrantRequestDenyItem] | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        workarea_node_id = self.workarea_node_id

        role = self.role.value

        deny: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.deny, Unset):
            deny = []
            for deny_item_data in self.deny:
                deny_item = deny_item_data.to_dict()
                deny.append(deny_item)

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "workarea_node_id": workarea_node_id,
                "role": role,
            }
        )
        if deny is not UNSET:
            field_dict["deny"] = deny

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.grant_request_deny_item import GrantRequestDenyItem

        d = dict(src_dict)
        workarea_node_id = d.pop("workarea_node_id")

        role = GrantRequestRole(d.pop("role"))

        _deny = d.pop("deny", UNSET)
        deny: list[GrantRequestDenyItem] | Unset = UNSET
        if _deny is not UNSET:
            deny = []
            for deny_item_data in _deny:
                deny_item = GrantRequestDenyItem.from_dict(deny_item_data)

                deny.append(deny_item)

        grant_request = cls(
            workarea_node_id=workarea_node_id,
            role=role,
            deny=deny,
        )

        return grant_request
