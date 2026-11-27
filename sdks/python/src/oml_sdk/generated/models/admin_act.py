from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.admin_act_class import AdminActClass
from ..models.admin_act_state import AdminActState
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.admin_act_grants_item import AdminActGrantsItem
    from ..models.admin_act_target import AdminActTarget


T = TypeVar("T", bound="AdminAct")


@_attrs_define
class AdminAct:
    """One admin act (ADR-0097 d2): the confirm screen of d23 reads target (the key's role, principal, used_by, the root's
    person as user, and principal_names_person where the principal spells a person's name) and grants. Since v0.8.6.

        Attributes:
            act_id (UUID):
            act (str):
            class_ (AdminActClass):
            state (AdminActState):
            target (AdminActTarget):
            grants (list[AdminActGrantsItem]): What a pending act's confirmation grants; empty once applied.
            reason (None | str):
            actor_key_id (None | UUID):
            actor_person (None | str):
            confirmed_by_person (None | str):
            sole_admin (bool):
            cause (None | str): upgrade, deactivation, settlement or recovery for an act no admin took; null otherwise.
            at (datetime.datetime):
            settled_at (datetime.datetime | None):
            expires_at (datetime.datetime | None): When a pending act expires as declined; null for a pending shred, which
                never expires (ADR-0097 d4).
            to_confirm (bool | Unset): Served on GET /v2/admin/acts alone: whether the calling admin person may confirm or
                review the act, as the To do count acts_to_confirm judges it: never their own act, a pending act taken at least
                seven days after their admin flag was accepted, and a widening act a sole_admin tenant took at once once that
                flag is seven days old (ADR-0097 d2, d10, d11). Since v0.8.6.
    """

    act_id: UUID
    act: str
    class_: AdminActClass
    state: AdminActState
    target: AdminActTarget
    grants: list[AdminActGrantsItem]
    reason: None | str
    actor_key_id: None | UUID
    actor_person: None | str
    confirmed_by_person: None | str
    sole_admin: bool
    cause: None | str
    at: datetime.datetime
    settled_at: datetime.datetime | None
    expires_at: datetime.datetime | None
    to_confirm: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        act_id = str(self.act_id)

        act = self.act

        class_ = self.class_.value

        state = self.state.value

        target = self.target.to_dict()

        grants = []
        for grants_item_data in self.grants:
            grants_item = grants_item_data.to_dict()
            grants.append(grants_item)

        reason: None | str
        reason = self.reason

        actor_key_id: None | str
        if isinstance(self.actor_key_id, UUID):
            actor_key_id = str(self.actor_key_id)
        else:
            actor_key_id = self.actor_key_id

        actor_person: None | str
        actor_person = self.actor_person

        confirmed_by_person: None | str
        confirmed_by_person = self.confirmed_by_person

        sole_admin = self.sole_admin

        cause: None | str
        cause = self.cause

        at = self.at.isoformat()

        settled_at: None | str
        if isinstance(self.settled_at, datetime.datetime):
            settled_at = self.settled_at.isoformat()
        else:
            settled_at = self.settled_at

        expires_at: None | str
        if isinstance(self.expires_at, datetime.datetime):
            expires_at = self.expires_at.isoformat()
        else:
            expires_at = self.expires_at

        to_confirm = self.to_confirm

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "act_id": act_id,
                "act": act,
                "class": class_,
                "state": state,
                "target": target,
                "grants": grants,
                "reason": reason,
                "actor_key_id": actor_key_id,
                "actor_person": actor_person,
                "confirmed_by_person": confirmed_by_person,
                "sole_admin": sole_admin,
                "cause": cause,
                "at": at,
                "settled_at": settled_at,
                "expires_at": expires_at,
            }
        )
        if to_confirm is not UNSET:
            field_dict["to_confirm"] = to_confirm

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.admin_act_grants_item import AdminActGrantsItem
        from ..models.admin_act_target import AdminActTarget

        d = dict(src_dict)
        act_id = UUID(d.pop("act_id"))

        act = d.pop("act")

        class_ = AdminActClass(d.pop("class"))

        state = AdminActState(d.pop("state"))

        target = AdminActTarget.from_dict(d.pop("target"))

        grants = []
        _grants = d.pop("grants")
        for grants_item_data in _grants:
            grants_item = AdminActGrantsItem.from_dict(grants_item_data)

            grants.append(grants_item)

        def _parse_reason(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        reason = _parse_reason(d.pop("reason"))

        def _parse_actor_key_id(data: object) -> None | UUID:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                actor_key_id_type_0 = UUID(data)

                return actor_key_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | UUID, data)

        actor_key_id = _parse_actor_key_id(d.pop("actor_key_id"))

        def _parse_actor_person(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        actor_person = _parse_actor_person(d.pop("actor_person"))

        def _parse_confirmed_by_person(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        confirmed_by_person = _parse_confirmed_by_person(d.pop("confirmed_by_person"))

        sole_admin = d.pop("sole_admin")

        def _parse_cause(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        cause = _parse_cause(d.pop("cause"))

        at = datetime.datetime.fromisoformat(d.pop("at"))

        def _parse_settled_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                settled_at_type_0 = datetime.datetime.fromisoformat(data)

                return settled_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        settled_at = _parse_settled_at(d.pop("settled_at"))

        def _parse_expires_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                expires_at_type_0 = datetime.datetime.fromisoformat(data)

                return expires_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        expires_at = _parse_expires_at(d.pop("expires_at"))

        to_confirm = d.pop("to_confirm", UNSET)

        admin_act = cls(
            act_id=act_id,
            act=act,
            class_=class_,
            state=state,
            target=target,
            grants=grants,
            reason=reason,
            actor_key_id=actor_key_id,
            actor_person=actor_person,
            confirmed_by_person=confirmed_by_person,
            sole_admin=sole_admin,
            cause=cause,
            at=at,
            settled_at=settled_at,
            expires_at=expires_at,
            to_confirm=to_confirm,
        )

        admin_act.additional_properties = d
        return admin_act

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
