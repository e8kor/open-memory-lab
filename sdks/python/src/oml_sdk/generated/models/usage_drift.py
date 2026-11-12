from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.usage_drift_drift_item import UsageDriftDriftItem
    from ..models.usage_drift_undeclared_profiles_item import (
        UsageDriftUndeclaredProfilesItem,
    )


T = TypeVar("T", bound="UsageDrift")


@_attrs_define
class UsageDrift:
    """
    Attributes:
        drift (list[UsageDriftDriftItem]):
        undeclared_profiles (list[UsageDriftUndeclaredProfilesItem]):
    """

    drift: list[UsageDriftDriftItem]
    undeclared_profiles: list[UsageDriftUndeclaredProfilesItem]

    def to_dict(self) -> dict[str, Any]:
        drift = []
        for drift_item_data in self.drift:
            drift_item = drift_item_data.to_dict()
            drift.append(drift_item)

        undeclared_profiles = []
        for undeclared_profiles_item_data in self.undeclared_profiles:
            undeclared_profiles_item = undeclared_profiles_item_data.to_dict()
            undeclared_profiles.append(undeclared_profiles_item)

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "drift": drift,
                "undeclared_profiles": undeclared_profiles,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.usage_drift_drift_item import UsageDriftDriftItem
        from ..models.usage_drift_undeclared_profiles_item import (
            UsageDriftUndeclaredProfilesItem,
        )

        d = dict(src_dict)
        drift = []
        _drift = d.pop("drift")
        for drift_item_data in _drift:
            drift_item = UsageDriftDriftItem.from_dict(drift_item_data)

            drift.append(drift_item)

        undeclared_profiles = []
        _undeclared_profiles = d.pop("undeclared_profiles")
        for undeclared_profiles_item_data in _undeclared_profiles:
            undeclared_profiles_item = UsageDriftUndeclaredProfilesItem.from_dict(
                undeclared_profiles_item_data
            )

            undeclared_profiles.append(undeclared_profiles_item)

        usage_drift = cls(
            drift=drift,
            undeclared_profiles=undeclared_profiles,
        )

        return usage_drift
