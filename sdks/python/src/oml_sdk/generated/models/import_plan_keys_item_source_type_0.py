from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="ImportPlanKeysItemSourceType0")


@_attrs_define
class ImportPlanKeysItemSourceType0:
    """
    Attributes:
        artifact_id (str):
        version (int):
    """

    artifact_id: str
    version: int

    def to_dict(self) -> dict[str, Any]:
        artifact_id = self.artifact_id

        version = self.version

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "artifact_id": artifact_id,
                "version": version,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        artifact_id = d.pop("artifact_id")

        version = d.pop("version")

        import_plan_keys_item_source_type_0 = cls(
            artifact_id=artifact_id,
            version=version,
        )

        return import_plan_keys_item_source_type_0
