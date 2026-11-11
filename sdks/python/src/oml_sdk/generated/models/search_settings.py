from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.postings_health import PostingsHealth
    from ..models.search_settings_valid_time import SearchSettingsValidTime


T = TypeVar("T", bound="SearchSettings")


@_attrs_define
class SearchSettings:
    """
    Attributes:
        enabled (list[str]): The pipelines the deployment serves (search.enabled).
        deployment_default (str): search.default.
        tenant_default (None | str): The tenant's journalled default; a name the deployment no longer serves stays here,
            and effective is then null.
        effective (None | str): The pipeline a recall naming none runs now; null while tenant_default names a pipeline
            the deployment does not serve, when such a recall is refused with settings.invalid (ADR-0049 d15).
        postings (PostingsHealth | Unset): The tenant's integer postings, as doctor reads them (ADR-0052 d12). Present
            on GET /v2/search where the deployment serves bm25-postings or a sparse pipeline; absent from the answer to POST
            /v2/search/default.
        valid_time (SearchSettingsValidTime | Unset): The tenant's served memories that declare valid_from, and those
            whose valid_until has passed, which recall hides by default (ADR-0059 d12).
    """

    enabled: list[str]
    deployment_default: str
    tenant_default: None | str
    effective: None | str
    postings: PostingsHealth | Unset = UNSET
    valid_time: SearchSettingsValidTime | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        enabled = self.enabled

        deployment_default = self.deployment_default

        tenant_default: None | str
        tenant_default = self.tenant_default

        effective: None | str
        effective = self.effective

        postings: dict[str, Any] | Unset = UNSET
        if not isinstance(self.postings, Unset):
            postings = self.postings.to_dict()

        valid_time: dict[str, Any] | Unset = UNSET
        if not isinstance(self.valid_time, Unset):
            valid_time = self.valid_time.to_dict()

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
        if valid_time is not UNSET:
            field_dict["valid_time"] = valid_time

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.postings_health import PostingsHealth
        from ..models.search_settings_valid_time import SearchSettingsValidTime

        d = dict(src_dict)
        enabled = cast(list[str], d.pop("enabled"))

        deployment_default = d.pop("deployment_default")

        def _parse_tenant_default(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        tenant_default = _parse_tenant_default(d.pop("tenant_default"))

        def _parse_effective(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        effective = _parse_effective(d.pop("effective"))

        _postings = d.pop("postings", UNSET)
        postings: PostingsHealth | Unset
        if isinstance(_postings, Unset):
            postings = UNSET
        else:
            postings = PostingsHealth.from_dict(_postings)

        _valid_time = d.pop("valid_time", UNSET)
        valid_time: SearchSettingsValidTime | Unset
        if isinstance(_valid_time, Unset):
            valid_time = UNSET
        else:
            valid_time = SearchSettingsValidTime.from_dict(_valid_time)

        search_settings = cls(
            enabled=enabled,
            deployment_default=deployment_default,
            tenant_default=tenant_default,
            effective=effective,
            postings=postings,
            valid_time=valid_time,
        )

        return search_settings
