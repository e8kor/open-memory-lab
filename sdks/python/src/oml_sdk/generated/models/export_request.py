from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.export_request_mode import ExportRequestMode
from ..types import UNSET, Unset

T = TypeVar("T", bound="ExportRequest")


@_attrs_define
class ExportRequest:
    """
    Attributes:
        recipients (list[str]): At least one; the bundle is encrypted to each.
        mode (ExportRequestMode | Unset): history is the default in a user root; release is refused until releases are
            built.
        release (str | Unset): A release label; refused until releases are built.
        since (str | Unset): A base release label; refused until releases are built.
    """

    recipients: list[str]
    mode: ExportRequestMode | Unset = UNSET
    release: str | Unset = UNSET
    since: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        recipients = self.recipients

        mode: str | Unset = UNSET
        if not isinstance(self.mode, Unset):
            mode = self.mode.value

        release = self.release

        since = self.since

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "recipients": recipients,
            }
        )
        if mode is not UNSET:
            field_dict["mode"] = mode
        if release is not UNSET:
            field_dict["release"] = release
        if since is not UNSET:
            field_dict["since"] = since

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        recipients = cast(list[str], d.pop("recipients"))

        _mode = d.pop("mode", UNSET)
        mode: ExportRequestMode | Unset
        if isinstance(_mode, Unset):
            mode = UNSET
        else:
            mode = ExportRequestMode(_mode)

        release = d.pop("release", UNSET)

        since = d.pop("since", UNSET)

        export_request = cls(
            recipients=recipients,
            mode=mode,
            release=release,
            since=since,
        )

        return export_request
