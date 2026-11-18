from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="ImportPlanSource")


@_attrs_define
class ImportPlanSource:
    """
    Attributes:
        server_id (str):
        tenant_id (str):
        tenant_name (str):
    """

    server_id: str
    tenant_id: str
    tenant_name: str

    def to_dict(self) -> dict[str, Any]:
        server_id = self.server_id

        tenant_id = self.tenant_id

        tenant_name = self.tenant_name

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "server_id": server_id,
                "tenant_id": tenant_id,
                "tenant_name": tenant_name,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        server_id = d.pop("server_id")

        tenant_id = d.pop("tenant_id")

        tenant_name = d.pop("tenant_name")

        import_plan_source = cls(
            server_id=server_id,
            tenant_id=tenant_id,
            tenant_name=tenant_name,
        )

        return import_plan_source
