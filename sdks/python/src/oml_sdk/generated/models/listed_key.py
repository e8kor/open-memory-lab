from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define

from ..models.listed_key_credential import ListedKeyCredential
from ..models.listed_key_held_by import ListedKeyHeldBy
from ..models.role import Role

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
        )

        return listed_key
