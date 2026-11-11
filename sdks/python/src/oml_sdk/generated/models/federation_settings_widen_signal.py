from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="FederationSettingsWidenSignal")


@_attrs_define
class FederationSettingsWidenSignal:
    """The widen hint's signals on this tenant since the lead started; doctor warns on a failed build, and on recalls that
    found none while none was built.

        Attributes:
            built (int): Signals built.
            unserved (int): Scope recalls answered without the hint because their signal was missing, older than 300 seconds
                or built under other grants.
            failed (int): Signal builds that failed.
            last_failure (None | str): The last failed build's reason.
    """

    built: int
    unserved: int
    failed: int
    last_failure: None | str

    def to_dict(self) -> dict[str, Any]:
        built = self.built

        unserved = self.unserved

        failed = self.failed

        last_failure: None | str
        last_failure = self.last_failure

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "built": built,
                "unserved": unserved,
                "failed": failed,
                "last_failure": last_failure,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        built = d.pop("built")

        unserved = d.pop("unserved")

        failed = d.pop("failed")

        def _parse_last_failure(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        last_failure = _parse_last_failure(d.pop("last_failure"))

        federation_settings_widen_signal = cls(
            built=built,
            unserved=unserved,
            failed=failed,
            last_failure=last_failure,
        )

        return federation_settings_widen_signal
