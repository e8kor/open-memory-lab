from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.grant_request_covers import GrantRequestCovers
from ..models.grant_request_role import GrantRequestRole
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.grant_request_deny_item import GrantRequestDenyItem


T = TypeVar("T", bound="GrantRequest")


@_attrs_define
class GrantRequest:
    """
    Attributes:
        workarea (str): A workarea, by its node id, its name or its qualified name. Resolved server-side before any
            check that reads state; an unknown name answers exactly what an unknown id answers. A user key resolves a bare
            name in its own user root's namespace, plus universe and its root's name; every other key in the shared
            namespace (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is universe,
            is neither, and is refused (request.validation).
        role (GrantRequestRole): A grant role, admissible under the key's role (the admissibility table in
            docs/architecture/roles-and-keys.md); never admin or operator.
        deny (list[GrantRequestDenyItem] | Unset): Per-grant masks; each masks a subtree within this grant only and must
            lie inside it (request.validation otherwise). A node grant takes none (request.validation, details.field
            /grants/<i>/deny).
        covers (GrantRequestCovers | Unset): subtree covers the node and every descendant; node covers the node alone
            (ADR-0044 d4). Default: GrantRequestCovers.SUBTREE.
    """

    workarea: str
    role: GrantRequestRole
    deny: list[GrantRequestDenyItem] | Unset = UNSET
    covers: GrantRequestCovers | Unset = GrantRequestCovers.SUBTREE

    def to_dict(self) -> dict[str, Any]:
        workarea: str
        workarea = self.workarea

        role = self.role.value

        deny: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.deny, Unset):
            deny = []
            for deny_item_data in self.deny:
                deny_item = deny_item_data.to_dict()
                deny.append(deny_item)

        covers: str | Unset = UNSET
        if not isinstance(self.covers, Unset):
            covers = self.covers.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "workarea": workarea,
                "role": role,
            }
        )
        if deny is not UNSET:
            field_dict["deny"] = deny
        if covers is not UNSET:
            field_dict["covers"] = covers

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.grant_request_deny_item import GrantRequestDenyItem

        d = dict(src_dict)

        def _parse_workarea(data: object) -> str:
            return cast(str, data)

        workarea = _parse_workarea(d.pop("workarea"))

        role = GrantRequestRole(d.pop("role"))

        _deny = d.pop("deny", UNSET)
        deny: list[GrantRequestDenyItem] | Unset = UNSET
        if _deny is not UNSET:
            deny = []
            for deny_item_data in _deny:
                deny_item = GrantRequestDenyItem.from_dict(deny_item_data)

                deny.append(deny_item)

        _covers = d.pop("covers", UNSET)
        covers: GrantRequestCovers | Unset
        if isinstance(_covers, Unset):
            covers = UNSET
        else:
            covers = GrantRequestCovers(_covers)

        grant_request = cls(
            workarea=workarea,
            role=role,
            deny=deny,
            covers=covers,
        )

        return grant_request
