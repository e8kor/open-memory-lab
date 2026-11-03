from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="GrantRequestDenyItem")


@_attrs_define
class GrantRequestDenyItem:
    """
    Attributes:
        subtree_node_id (str): A workarea, by its node id or its name. Resolved server-side before any check that reads
            state; an unknown name answers exactly what an unknown id answers. A derived path is neither, and is refused
            (request.validation).
    """

    subtree_node_id: str

    def to_dict(self) -> dict[str, Any]:
        subtree_node_id: str
        subtree_node_id = self.subtree_node_id

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "subtree_node_id": subtree_node_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_subtree_node_id(data: object) -> str:
            return cast(str, data)

        subtree_node_id = _parse_subtree_node_id(d.pop("subtree_node_id"))

        grant_request_deny_item = cls(
            subtree_node_id=subtree_node_id,
        )

        return grant_request_deny_item
