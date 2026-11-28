from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.federation_settings_default_sources import (
    FederationSettingsDefaultSources,
)
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.federation_settings_wide_keys_item import (
        FederationSettingsWideKeysItem,
    )
    from ..models.federation_settings_widen_signal import FederationSettingsWidenSignal


T = TypeVar("T", bound="FederationSettings")


@_attrs_define
class FederationSettings:
    """The tenant's federation policy (ADR-0072 d5), and for doctor the keys whose recall met more than 1,000 candidate
    sources since the lead started (d12) and how the widen hint's signal fared (d8).

        Attributes:
            enabled (bool): Whether recall may search beyond its scope when asked; off by default.
            default_sources (FederationSettingsDefaultSources): What an absent sources reads; never a list.
            max_sources (int): The most workspaces a wide recall searches beside its scope, and the most a list names; 8 by
                default.
            wide_keys (list[FederationSettingsWideKeysItem]): The wide keys, listed to an admin key; empty to a maintenance
                key, which reads the counts below (ADR-0097 d18).
            widen_signal (FederationSettingsWidenSignal): The widen hint's signals on this tenant since the lead started;
                doctor warns on a failed build, on evictions, and on recalls that found none while none was built.
            wide_key_count (int | Unset): How many keys met more than 1,000 candidate sources since the lead started, to an
                admin and a maintenance key alike (ADR-0097 d18). Since v0.8.6.
            wide_key_most_candidates (int | Unset): The most candidate sources any of those keys met; 0 with none. Since
                v0.8.6.
    """

    enabled: bool
    default_sources: FederationSettingsDefaultSources
    max_sources: int
    wide_keys: list[FederationSettingsWideKeysItem]
    widen_signal: FederationSettingsWidenSignal
    wide_key_count: int | Unset = UNSET
    wide_key_most_candidates: int | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        enabled = self.enabled

        default_sources = self.default_sources.value

        max_sources = self.max_sources

        wide_keys = []
        for wide_keys_item_data in self.wide_keys:
            wide_keys_item = wide_keys_item_data.to_dict()
            wide_keys.append(wide_keys_item)

        widen_signal = self.widen_signal.to_dict()

        wide_key_count = self.wide_key_count

        wide_key_most_candidates = self.wide_key_most_candidates

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "enabled": enabled,
                "default_sources": default_sources,
                "max_sources": max_sources,
                "wide_keys": wide_keys,
                "widen_signal": widen_signal,
            }
        )
        if wide_key_count is not UNSET:
            field_dict["wide_key_count"] = wide_key_count
        if wide_key_most_candidates is not UNSET:
            field_dict["wide_key_most_candidates"] = wide_key_most_candidates

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.federation_settings_wide_keys_item import (
            FederationSettingsWideKeysItem,
        )
        from ..models.federation_settings_widen_signal import (
            FederationSettingsWidenSignal,
        )

        d = dict(src_dict)
        enabled = d.pop("enabled")

        default_sources = FederationSettingsDefaultSources(d.pop("default_sources"))

        max_sources = d.pop("max_sources")

        wide_keys = []
        _wide_keys = d.pop("wide_keys")
        for wide_keys_item_data in _wide_keys:
            wide_keys_item = FederationSettingsWideKeysItem.from_dict(
                wide_keys_item_data
            )

            wide_keys.append(wide_keys_item)

        widen_signal = FederationSettingsWidenSignal.from_dict(d.pop("widen_signal"))

        wide_key_count = d.pop("wide_key_count", UNSET)

        wide_key_most_candidates = d.pop("wide_key_most_candidates", UNSET)

        federation_settings = cls(
            enabled=enabled,
            default_sources=default_sources,
            max_sources=max_sources,
            wide_keys=wide_keys,
            widen_signal=widen_signal,
            wide_key_count=wide_key_count,
            wide_key_most_candidates=wide_key_most_candidates,
        )

        return federation_settings
