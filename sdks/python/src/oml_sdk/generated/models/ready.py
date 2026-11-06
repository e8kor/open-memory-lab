from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.ready_status import ReadyStatus
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.build import Build
    from ..models.ready_checks import ReadyChecks
    from ..models.ready_scan_type_0 import ReadyScanType0


T = TypeVar("T", bound="Ready")


@_attrs_define
class Ready:
    """
    Attributes:
        status (ReadyStatus):
        checks (ReadyChecks): Per-check detail, e.g. postgres, fork_check.
        oauth_issuer (None | str): The lead's oauth.issuer, or null where connections are off; doctor reads whether
            connections are on here, not from the discovery paths it checks (ADR-0031 d14).
        build (Build): The build that answered (http-api.md "Health"). Times are UTC, YYYY-MM-DDTHH:MM:SSZ; a field
            nothing could supply is the string unknown.
        scan (None | ReadyScanType0 | Unset): The scanner's rule set, how many rules it loaded and whether the personal-
            data rules are on, or null for a server that pins none; doctor's scan line reads it (ADR-0065 d11). Its findings
            are counted on the metrics listener, never here.
    """

    status: ReadyStatus
    checks: ReadyChecks
    oauth_issuer: None | str
    build: Build
    scan: None | ReadyScanType0 | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.ready_scan_type_0 import ReadyScanType0

        status = self.status.value

        checks = self.checks.to_dict()

        oauth_issuer: None | str
        oauth_issuer = self.oauth_issuer

        build = self.build.to_dict()

        scan: dict[str, Any] | None | Unset
        if isinstance(self.scan, Unset):
            scan = UNSET
        elif isinstance(self.scan, ReadyScanType0):
            scan = self.scan.to_dict()
        else:
            scan = self.scan

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "status": status,
                "checks": checks,
                "oauth_issuer": oauth_issuer,
                "build": build,
            }
        )
        if scan is not UNSET:
            field_dict["scan"] = scan

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.build import Build
        from ..models.ready_checks import ReadyChecks
        from ..models.ready_scan_type_0 import ReadyScanType0

        d = dict(src_dict)
        status = ReadyStatus(d.pop("status"))

        checks = ReadyChecks.from_dict(d.pop("checks"))

        def _parse_oauth_issuer(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        oauth_issuer = _parse_oauth_issuer(d.pop("oauth_issuer"))

        build = Build.from_dict(d.pop("build"))

        def _parse_scan(data: object) -> None | ReadyScanType0 | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                scan_type_0 = ReadyScanType0.from_dict(data)

                return scan_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ReadyScanType0 | Unset, data)

        scan = _parse_scan(d.pop("scan", UNSET))

        ready = cls(
            status=status,
            checks=checks,
            oauth_issuer=oauth_issuer,
            build=build,
            scan=scan,
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
