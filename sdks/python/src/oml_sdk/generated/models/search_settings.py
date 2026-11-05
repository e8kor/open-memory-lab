from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.postings_health import PostingsHealth


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
        postings (PostingsHealth | Unset): The tenant's integer postings, as doctor reads them (ADR-0052 d12). Present
            on GET /v2/search where the deployment serves bm25-postings; absent from the answer to POST /v2/search/default.
    """

    enabled: list[str]
    deployment_default: str
    tenant_default: None | str
    effective: str
    postings: PostingsHealth | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        enabled = self.enabled

        deployment_default = self.deployment_default

        tenant_default: None | str
        tenant_default = self.tenant_default

        effective = self.effective

        postings: dict[str, Any] | Unset = UNSET
        if not isinstance(self.postings, Unset):
            postings = self.postings.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "enabled": enabled,
                "deployment_default": deployment_default,
                "tenant_default": tenant_default,
                "effective": effective,
            }
        )
        if postings is not UNSET:
            field_dict["postings"] = postings

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.postings_health import PostingsHealth

        d = dict(src_dict)
        enabled = cast(list[str], d.pop("enabled"))

        deployment_default = d.pop("deployment_default")

        def _parse_tenant_default(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        tenant_default = _parse_tenant_default(d.pop("tenant_default"))

        effective = d.pop("effective")

        _postings = d.pop("postings", UNSET)
        postings: PostingsHealth | Unset
        if isinstance(_postings, Unset):
            postings = UNSET
        else:
            postings = PostingsHealth.from_dict(_postings)

        search_settings = cls(
            enabled=enabled,
            deployment_default=deployment_default,
            tenant_default=tenant_default,
            effective=effective,
            postings=postings,
        )

        return search_settings
