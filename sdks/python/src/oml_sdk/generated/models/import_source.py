from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="ImportSource")


@_attrs_define
class ImportSource:
    """
    Attributes:
        server_id (str):
        tenant_id (str):
    """

    server_id: str
    tenant_id: str

    def to_dict(self) -> dict[str, Any]:
        server_id = self.server_id

        tenant_id = self.tenant_id

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "server_id": server_id,
                "tenant_id": tenant_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        server_id = d.pop("server_id")

        tenant_id = d.pop("tenant_id")

        import_source = cls(
            server_id=server_id,
            tenant_id=tenant_id,
        )

        return import_source
