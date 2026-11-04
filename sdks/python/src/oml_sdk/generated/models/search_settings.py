from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="SearchSettings")


@_attrs_define
class SearchSettings:
    """
    Attributes:
        enabled (list[str]): The pipelines the deployment serves (search.enabled).
        deployment_default (str): search.default.
        tenant_default (None | str): The tenant's journalled default; a name the deployment no longer serves stays here
            and is not effective.
        effective (str): The pipeline a recall naming none runs now.
    """

    enabled: list[str]
    deployment_default: str
    tenant_default: None | str
    effective: str

    def to_dict(self) -> dict[str, Any]:
        enabled = self.enabled

        deployment_default = self.deployment_default

        tenant_default: None | str
        tenant_default = self.tenant_default

        effective = self.effective

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "enabled": enabled,
                "deployment_default": deployment_default,
                "tenant_default": tenant_default,
                "effective": effective,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        enabled = cast(list[str], d.pop("enabled"))

        deployment_default = d.pop("deployment_default")

        def _parse_tenant_default(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        tenant_default = _parse_tenant_default(d.pop("tenant_default"))

        effective = d.pop("effective")

        search_settings = cls(
            enabled=enabled,
            deployment_default=deployment_default,
            tenant_default=tenant_default,
            effective=effective,
        )

        return search_settings
