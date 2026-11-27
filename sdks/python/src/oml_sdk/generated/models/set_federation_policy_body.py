from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.set_federation_policy_body_default_sources import (
    SetFederationPolicyBodyDefaultSources,
)
from ..types import UNSET, Unset

T = TypeVar("T", bound="SetFederationPolicyBody")


@_attrs_define
class SetFederationPolicyBody:
    """
    Attributes:
        enabled (bool | Unset):
        default_sources (SetFederationPolicyBodyDefaultSources | Unset):
        max_sources (int | Unset):
        reason (str | Unset): The admin act's reason, scanned as a verdict's; a widening that waits requires one
            (ADR-0097 d26). Since v0.8.6.
    """

    enabled: bool | Unset = UNSET
    default_sources: SetFederationPolicyBodyDefaultSources | Unset = UNSET
    max_sources: int | Unset = UNSET
    reason: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        enabled = self.enabled

        default_sources: str | Unset = UNSET
        if not isinstance(self.default_sources, Unset):
            default_sources = self.default_sources.value

        max_sources = self.max_sources

        reason = self.reason

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if enabled is not UNSET:
            field_dict["enabled"] = enabled
        if default_sources is not UNSET:
            field_dict["default_sources"] = default_sources
        if max_sources is not UNSET:
            field_dict["max_sources"] = max_sources
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        enabled = d.pop("enabled", UNSET)

        _default_sources = d.pop("default_sources", UNSET)
        default_sources: SetFederationPolicyBodyDefaultSources | Unset
        if isinstance(_default_sources, Unset):
            default_sources = UNSET
        else:
            default_sources = SetFederationPolicyBodyDefaultSources(_default_sources)

        max_sources = d.pop("max_sources", UNSET)

        reason = d.pop("reason", UNSET)

        set_federation_policy_body = cls(
            enabled=enabled,
            default_sources=default_sources,
            max_sources=max_sources,
            reason=reason,
        )

        return set_federation_policy_body
