from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.ready_status import ReadyStatus

if TYPE_CHECKING:
    from ..models.build import Build
    from ..models.ready_checks import ReadyChecks


T = TypeVar("T", bound="Ready")


@_attrs_define
class Ready:
    """
    Attributes:
        status (ReadyStatus):
        checks (ReadyChecks): Per-check detail, e.g. postgres, fork_check.
        build (Build): The build that answered (http-api.md "Health"). Times are UTC, YYYY-MM-DDTHH:MM:SSZ; a field
            nothing could supply is the string unknown.
    """

    status: ReadyStatus
    checks: ReadyChecks
    build: Build
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        status = self.status.value

        checks = self.checks.to_dict()

        build = self.build.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "status": status,
                "checks": checks,
                "build": build,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.build import Build
        from ..models.ready_checks import ReadyChecks

        d = dict(src_dict)
        status = ReadyStatus(d.pop("status"))

        checks = ReadyChecks.from_dict(d.pop("checks"))

        build = Build.from_dict(d.pop("build"))

        ready = cls(
            status=status,
            checks=checks,
            build=build,
        )

        ready.additional_properties = d
        return ready

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
