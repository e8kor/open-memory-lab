from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="NewWorkspaceRequest")


@_attrs_define
class NewWorkspaceRequest:
    """A workspace the issuance creates in the same act (ADR-0097 d30). Since v0.8.6.

    Attributes:
        parent (str): A workspace, by its node id, its name or its qualified name. Resolved server-side before any check
            that reads state; an unknown name answers exactly what an unknown id answers. A user key resolves a bare name in
            its own user root's namespace, plus universe and its root's name; every other key in the shared namespace
            (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is universe, is neither,
            and is refused (request.validation).
        name (str): A new name, in a creation body: the creation grammar. A lowercase RFC 1035 label, 1-63 characters,
            that is not in the id form, not 32 hexadecimal digits and not the derived form, which only rows that predate
            names hold; a creation that sends that form is request.validation. Unique in its namespace, immutable, never
            released; compared byte for byte, never lowercased (docs/api/http-api.md, Names and ids; ADR-0026 d1, d2).
            References use NameRef.
    """

    parent: str
    name: str

    def to_dict(self) -> dict[str, Any]:
        parent: str
        parent = self.parent

        name = self.name

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "parent": parent,
                "name": name,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_parent(data: object) -> str:
            return cast(str, data)

        parent = _parse_parent(d.pop("parent"))

        name = d.pop("name")

        new_workspace_request = cls(
            parent=parent,
            name=name,
        )

        return new_workspace_request
