from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define

from ..models.listed_key_credential import ListedKeyCredential
from ..models.listed_key_held_by import ListedKeyHeldBy
from ..models.role import Role
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.listed_key_grants_item import ListedKeyGrantsItem


T = TypeVar("T", bound="ListedKey")


@_attrs_define
class ListedKey:
    """
    Attributes:
        id (UUID):
        name (None | str):
        role (Role):
        held_by (ListedKeyHeldBy):
        credential (ListedKeyCredential):
        user_root_id (None | UUID):
        revoked (bool):
        cut_off (bool): A denial on universe cuts the key off (ADR-0045 d9).
        grants (list[ListedKeyGrantsItem]):
        expiry (datetime.datetime | None | Unset): The instant from which the key authenticates nothing; null for a key
            that does not expire (ADR-0079 d3; since v0.7.3).
        expired (bool | Unset): Whether the expiry has come on the server's clock (ADR-0079 d3; since v0.7.3).
        requests_per_minute (int | None | Unset): The key's own requests per UTC minute; null for none of its own
            (ADR-0079 d4; since v0.7.3).
    """

    id: UUID
    name: None | str
    role: Role
    held_by: ListedKeyHeldBy
    credential: ListedKeyCredential
    user_root_id: None | UUID
    revoked: bool
    cut_off: bool
    grants: list[ListedKeyGrantsItem]
    expiry: datetime.datetime | None | Unset = UNSET
    expired: bool | Unset = UNSET
    requests_per_minute: int | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        id = str(self.id)

        name: None | str
        name = self.name

        role = self.role.value

        held_by = self.held_by.value

        credential = self.credential.value

        user_root_id: None | str
        if isinstance(self.user_root_id, UUID):
            user_root_id = str(self.user_root_id)
        else:
            user_root_id = self.user_root_id

        revoked = self.revoked

        cut_off = self.cut_off

        grants = []
        for grants_item_data in self.grants:
            grants_item = grants_item_data.to_dict()
            grants.append(grants_item)

        expiry: None | str | Unset
        if isinstance(self.expiry, Unset):
            expiry = UNSET
        elif isinstance(self.expiry, datetime.datetime):
            expiry = self.expiry.isoformat()
        else:
            expiry = self.expiry

        expired = self.expired

        requests_per_minute: int | None | Unset
        if isinstance(self.requests_per_minute, Unset):
            requests_per_minute = UNSET
        else:
            requests_per_minute = self.requests_per_minute

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "id": id,
                "name": name,
                "role": role,
                "held_by": held_by,
                "credential": credential,
                "user_root_id": user_root_id,
                "revoked": revoked,
                "cut_off": cut_off,
                "grants": grants,
            }
        )
        if expiry is not UNSET:
            field_dict["expiry"] = expiry
        if expired is not UNSET:
            field_dict["expired"] = expired
        if requests_per_minute is not UNSET:
            field_dict["requests_per_minute"] = requests_per_minute

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.listed_key_grants_item import ListedKeyGrantsItem

        d = dict(src_dict)
        id = UUID(d.pop("id"))

        def _parse_name(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        name = _parse_name(d.pop("name"))

        role = Role(d.pop("role"))

        held_by = ListedKeyHeldBy(d.pop("held_by"))

        credential = ListedKeyCredential(d.pop("credential"))

        def _parse_user_root_id(data: object) -> None | UUID:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                user_root_id_type_0 = UUID(data)

                return user_root_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | UUID, data)

        user_root_id = _parse_user_root_id(d.pop("user_root_id"))

        revoked = d.pop("revoked")

        cut_off = d.pop("cut_off")

        grants = []
        _grants = d.pop("grants")
        for grants_item_data in _grants:
            grants_item = ListedKeyGrantsItem.from_dict(grants_item_data)

            grants.append(grants_item)

        def _parse_expiry(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                expiry_type_0 = datetime.datetime.fromisoformat(data)

                return expiry_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        expiry = _parse_expiry(d.pop("expiry", UNSET))

        expired = d.pop("expired", UNSET)

        def _parse_requests_per_minute(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        requests_per_minute = _parse_requests_per_minute(
            d.pop("requests_per_minute", UNSET)
        )

        listed_key = cls(
            id=id,
            name=name,
            role=role,
            held_by=held_by,
            credential=credential,
            user_root_id=user_root_id,
            revoked=revoked,
            cut_off=cut_off,
            grants=grants,
            expiry=expiry,
            expired=expired,
            requests_per_minute=requests_per_minute,
        )

        return listed_key
