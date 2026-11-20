from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.whoami_person_pending_emails_item_added_by import (
        WhoamiPersonPendingEmailsItemAddedBy,
    )


T = TypeVar("T", bound="WhoamiPersonPendingEmailsItem")


@_attrs_define
class WhoamiPersonPendingEmailsItem:
    """
    Attributes:
        email (str):
        admin (bool):
        added_by (WhoamiPersonPendingEmailsItemAddedBy):
    """

    email: str
    admin: bool
    added_by: WhoamiPersonPendingEmailsItemAddedBy

    def to_dict(self) -> dict[str, Any]:
        email = self.email

        admin = self.admin

        added_by = self.added_by.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "email": email,
                "admin": admin,
                "added_by": added_by,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.whoami_person_pending_emails_item_added_by import (
            WhoamiPersonPendingEmailsItemAddedBy,
        )

        d = dict(src_dict)
        email = d.pop("email")

        admin = d.pop("admin")

        added_by = WhoamiPersonPendingEmailsItemAddedBy.from_dict(d.pop("added_by"))

        whoami_person_pending_emails_item = cls(
            email=email,
            admin=admin,
            added_by=added_by,
        )

        return whoami_person_pending_emails_item
