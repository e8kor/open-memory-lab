from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.whoami_person_quotas_type_0 import WhoamiPersonQuotasType0
    from ..models.whoami_person_start_type_0 import WhoamiPersonStartType0
    from ..models.whoami_person_usage_type_0 import WhoamiPersonUsageType0


T = TypeVar("T", bound="WhoamiPerson")


@_attrs_define
class WhoamiPerson:
    """
    Attributes:
        user (str):
        under_quotas (bool): Whether the person is under the sign-up tenant's quotas; usage and quotas are null when not
            (ADR-0068 d5).
        usage (None | WhoamiPersonUsageType0):
        quotas (None | WhoamiPersonQuotasType0):
        deletion_requested_at (datetime.datetime | None): When the person asked for deletion, while the request is
            pending (ADR-0068 d6).
        start (None | Unset | WhoamiPersonStartType0): With start=true, what the onboarding page waits for: whether one
            of the person's agent keys has connected, and the state of the newest memory of their root, held or promoted,
            else null (ADR-0068 d10; ADR-0070 d5). Absent otherwise. Since v0.7.0.
    """

    user: str
    under_quotas: bool
    usage: None | WhoamiPersonUsageType0
    quotas: None | WhoamiPersonQuotasType0
    deletion_requested_at: datetime.datetime | None
    start: None | Unset | WhoamiPersonStartType0 = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.whoami_person_quotas_type_0 import WhoamiPersonQuotasType0
        from ..models.whoami_person_start_type_0 import WhoamiPersonStartType0
        from ..models.whoami_person_usage_type_0 import WhoamiPersonUsageType0

        user = self.user

        under_quotas = self.under_quotas

        usage: dict[str, Any] | None
        if isinstance(self.usage, WhoamiPersonUsageType0):
            usage = self.usage.to_dict()
        else:
            usage = self.usage

        quotas: dict[str, Any] | None
        if isinstance(self.quotas, WhoamiPersonQuotasType0):
            quotas = self.quotas.to_dict()
        else:
            quotas = self.quotas

        deletion_requested_at: None | str
        if isinstance(self.deletion_requested_at, datetime.datetime):
            deletion_requested_at = self.deletion_requested_at.isoformat()
        else:
            deletion_requested_at = self.deletion_requested_at

        start: dict[str, Any] | None | Unset
        if isinstance(self.start, Unset):
            start = UNSET
        elif isinstance(self.start, WhoamiPersonStartType0):
            start = self.start.to_dict()
        else:
            start = self.start

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "user": user,
                "under_quotas": under_quotas,
                "usage": usage,
                "quotas": quotas,
                "deletion_requested_at": deletion_requested_at,
            }
        )
        if start is not UNSET:
            field_dict["start"] = start

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.whoami_person_quotas_type_0 import WhoamiPersonQuotasType0
        from ..models.whoami_person_start_type_0 import WhoamiPersonStartType0
        from ..models.whoami_person_usage_type_0 import WhoamiPersonUsageType0

        d = dict(src_dict)
        user = d.pop("user")

        under_quotas = d.pop("under_quotas")

        def _parse_usage(data: object) -> None | WhoamiPersonUsageType0:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                usage_type_0 = WhoamiPersonUsageType0.from_dict(data)

                return usage_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | WhoamiPersonUsageType0, data)

        usage = _parse_usage(d.pop("usage"))

        def _parse_quotas(data: object) -> None | WhoamiPersonQuotasType0:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                quotas_type_0 = WhoamiPersonQuotasType0.from_dict(data)

                return quotas_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | WhoamiPersonQuotasType0, data)

        quotas = _parse_quotas(d.pop("quotas"))

        def _parse_deletion_requested_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                deletion_requested_at_type_0 = datetime.datetime.fromisoformat(data)

                return deletion_requested_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        deletion_requested_at = _parse_deletion_requested_at(
            d.pop("deletion_requested_at")
        )

        def _parse_start(data: object) -> None | Unset | WhoamiPersonStartType0:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                start_type_0 = WhoamiPersonStartType0.from_dict(data)

                return start_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | WhoamiPersonStartType0, data)

        start = _parse_start(d.pop("start", UNSET))

        whoami_person = cls(
            user=user,
            under_quotas=under_quotas,
            usage=usage,
            quotas=quotas,
            deletion_requested_at=deletion_requested_at,
            start=start,
        )

        return whoami_person
