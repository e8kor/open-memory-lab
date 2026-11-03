from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.export_job_kind import ExportJobKind
from ..models.export_job_state import ExportJobState
from ..types import UNSET, Unset

T = TypeVar("T", bound="ExportJob")


@_attrs_define
class ExportJob:
    """
    Attributes:
        export_id (str):
        kind (ExportJobKind):
        state (ExportJobState):
        requested_at (datetime.datetime | Unset):
        finished_at (datetime.datetime | None | Unset):
        bundle_bytes (int | None | Unset):
    """

    export_id: str
    kind: ExportJobKind
    state: ExportJobState
    requested_at: datetime.datetime | Unset = UNSET
    finished_at: datetime.datetime | None | Unset = UNSET
    bundle_bytes: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        export_id = self.export_id

        kind = self.kind.value

        state = self.state.value

        requested_at: str | Unset = UNSET
        if not isinstance(self.requested_at, Unset):
            requested_at = self.requested_at.isoformat()

        finished_at: None | str | Unset
        if isinstance(self.finished_at, Unset):
            finished_at = UNSET
        elif isinstance(self.finished_at, datetime.datetime):
            finished_at = self.finished_at.isoformat()
        else:
            finished_at = self.finished_at

        bundle_bytes: int | None | Unset
        if isinstance(self.bundle_bytes, Unset):
            bundle_bytes = UNSET
        else:
            bundle_bytes = self.bundle_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "export_id": export_id,
                "kind": kind,
                "state": state,
            }
        )
        if requested_at is not UNSET:
            field_dict["requested_at"] = requested_at
        if finished_at is not UNSET:
            field_dict["finished_at"] = finished_at
        if bundle_bytes is not UNSET:
            field_dict["bundle_bytes"] = bundle_bytes

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        export_id = d.pop("export_id")

        kind = ExportJobKind(d.pop("kind"))

        state = ExportJobState(d.pop("state"))

        _requested_at = d.pop("requested_at", UNSET)
        requested_at: datetime.datetime | Unset
        if isinstance(_requested_at, Unset):
            requested_at = UNSET
        else:
            requested_at = datetime.datetime.fromisoformat(_requested_at)

        def _parse_finished_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                finished_at_type_0 = datetime.datetime.fromisoformat(data)

                return finished_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        finished_at = _parse_finished_at(d.pop("finished_at", UNSET))

        def _parse_bundle_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        bundle_bytes = _parse_bundle_bytes(d.pop("bundle_bytes", UNSET))

        export_job = cls(
            export_id=export_id,
            kind=kind,
            state=state,
            requested_at=requested_at,
            finished_at=finished_at,
            bundle_bytes=bundle_bytes,
        )

        export_job.additional_properties = d
        return export_job

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
