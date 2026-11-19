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
    from ..models.whoami_ref import WhoamiRef


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
        linked_keys (list[WhoamiRef] | Unset): The live tenant keys an admin linked to the person (not revoked, not past
            their expiry, used_by person), by id and name, in name order: the keys oml-web may name in OML-Acting-Key for
            this person (ADR-0085 d16). Since v0.8.0.
        pending_links (list[WhoamiRef] | Unset): The live tenant keys an admin linked to the person that wait on the
            person's acceptance, by id and name, in name order; each acts for no one until the person accepts it with POST
            /v2/keys/{key}/accept_link (ADR-0085 d16, the operator's decision of 2026-11-19). Since v0.8.4.
        pending_emails (list[str] | Unset): The sign-in emails an admin added for the person that wait on the person's
            acceptance, in email order; each signs in to nothing until the person accepts it with POST /v2/web/emails/accept
            or removes it with POST /v2/web/emails/decline (ADR-0045 d2, the operator's decision of 2026-11-19). Since
            v0.8.4.
        start (None | Unset | WhoamiPersonStartType0): With start=true, what the onboarding page waits for: whether one
            of the person's agent keys has connected, and the state of the newest memory of their root, held or promoted,
            else null (ADR-0068 d10; ADR-0070 d5). Absent otherwise. Since v0.7.0.
    """

    user: str
    under_quotas: bool
    usage: None | WhoamiPersonUsageType0
    quotas: None | WhoamiPersonQuotasType0
    deletion_requested_at: datetime.datetime | None
    linked_keys: list[WhoamiRef] | Unset = UNSET
    pending_links: list[WhoamiRef] | Unset = UNSET
    pending_emails: list[str] | Unset = UNSET
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

        linked_keys: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.linked_keys, Unset):
            linked_keys = []
            for linked_keys_item_data in self.linked_keys:
                linked_keys_item = linked_keys_item_data.to_dict()
                linked_keys.append(linked_keys_item)

        pending_links: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.pending_links, Unset):
            pending_links = []
            for pending_links_item_data in self.pending_links:
                pending_links_item = pending_links_item_data.to_dict()
                pending_links.append(pending_links_item)

        pending_emails: list[str] | Unset = UNSET
        if not isinstance(self.pending_emails, Unset):
            pending_emails = self.pending_emails

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
        if linked_keys is not UNSET:
            field_dict["linked_keys"] = linked_keys
        if pending_links is not UNSET:
            field_dict["pending_links"] = pending_links
        if pending_emails is not UNSET:
            field_dict["pending_emails"] = pending_emails
        if start is not UNSET:
            field_dict["start"] = start

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.whoami_person_quotas_type_0 import WhoamiPersonQuotasType0
        from ..models.whoami_person_start_type_0 import WhoamiPersonStartType0
        from ..models.whoami_person_usage_type_0 import WhoamiPersonUsageType0
        from ..models.whoami_ref import WhoamiRef

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

        _linked_keys = d.pop("linked_keys", UNSET)
        linked_keys: list[WhoamiRef] | Unset = UNSET
        if _linked_keys is not UNSET:
            linked_keys = []
            for linked_keys_item_data in _linked_keys:
                linked_keys_item = WhoamiRef.from_dict(linked_keys_item_data)

                linked_keys.append(linked_keys_item)

        _pending_links = d.pop("pending_links", UNSET)
        pending_links: list[WhoamiRef] | Unset = UNSET
        if _pending_links is not UNSET:
            pending_links = []
            for pending_links_item_data in _pending_links:
                pending_links_item = WhoamiRef.from_dict(pending_links_item_data)

                pending_links.append(pending_links_item)

        pending_emails = cast(list[str], d.pop("pending_emails", UNSET))

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
            linked_keys=linked_keys,
            pending_links=pending_links,
            pending_emails=pending_emails,
            start=start,
        )

        return whoami_person
