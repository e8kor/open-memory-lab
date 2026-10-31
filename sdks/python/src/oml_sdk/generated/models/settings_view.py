from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.settings_document import SettingsDocument
    from ..models.settings_view_divergences_item import SettingsViewDivergencesItem


T = TypeVar("T", bound="SettingsView")


@_attrs_define
class SettingsView:
    """
    Attributes:
        configured (SettingsDocument | Unset): The settings document; its schema is not fixed by this design — the
            contract is show/validate/apply and the retired-key refusal.
        applied_at (datetime.datetime | Unset):
        applied_event_id (str | Unset):
        effective (SettingsDocument | Unset): The settings document; its schema is not fixed by this design — the
            contract is show/validate/apply and the retired-key refusal.
        divergences (list[SettingsViewDivergencesItem] | Unset):
    """

    configured: SettingsDocument | Unset = UNSET
    applied_at: datetime.datetime | Unset = UNSET
    applied_event_id: str | Unset = UNSET
    effective: SettingsDocument | Unset = UNSET
    divergences: list[SettingsViewDivergencesItem] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        configured: dict[str, Any] | Unset = UNSET
        if not isinstance(self.configured, Unset):
            configured = self.configured.to_dict()

        applied_at: str | Unset = UNSET
        if not isinstance(self.applied_at, Unset):
            applied_at = self.applied_at.isoformat()

        applied_event_id = self.applied_event_id

        effective: dict[str, Any] | Unset = UNSET
        if not isinstance(self.effective, Unset):
            effective = self.effective.to_dict()

        divergences: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.divergences, Unset):
            divergences = []
            for divergences_item_data in self.divergences:
                divergences_item = divergences_item_data.to_dict()
                divergences.append(divergences_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if configured is not UNSET:
            field_dict["configured"] = configured
        if applied_at is not UNSET:
            field_dict["applied_at"] = applied_at
        if applied_event_id is not UNSET:
            field_dict["applied_event_id"] = applied_event_id
        if effective is not UNSET:
            field_dict["effective"] = effective
        if divergences is not UNSET:
            field_dict["divergences"] = divergences

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.settings_document import SettingsDocument
        from ..models.settings_view_divergences_item import SettingsViewDivergencesItem

        d = dict(src_dict)
        _configured = d.pop("configured", UNSET)
        configured: SettingsDocument | Unset
        if isinstance(_configured, Unset):
            configured = UNSET
        else:
            configured = SettingsDocument.from_dict(_configured)

        _applied_at = d.pop("applied_at", UNSET)
        applied_at: datetime.datetime | Unset
        if isinstance(_applied_at, Unset):
            applied_at = UNSET
        else:
            applied_at = datetime.datetime.fromisoformat(_applied_at)

        applied_event_id = d.pop("applied_event_id", UNSET)

        _effective = d.pop("effective", UNSET)
        effective: SettingsDocument | Unset
        if isinstance(_effective, Unset):
            effective = UNSET
        else:
            effective = SettingsDocument.from_dict(_effective)

        _divergences = d.pop("divergences", UNSET)
        divergences: list[SettingsViewDivergencesItem] | Unset = UNSET
        if _divergences is not UNSET:
            divergences = []
            for divergences_item_data in _divergences:
                divergences_item = SettingsViewDivergencesItem.from_dict(
                    divergences_item_data
                )

                divergences.append(divergences_item)

        settings_view = cls(
            configured=configured,
            applied_at=applied_at,
            applied_event_id=applied_event_id,
            effective=effective,
            divergences=divergences,
        )

        settings_view.additional_properties = d
        return settings_view

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
