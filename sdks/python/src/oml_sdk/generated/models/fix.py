from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.fix_act import FixAct
from ..models.fix_acts_item import FixActsItem
from ..models.fix_state import FixState
from ..types import UNSET, Unset

T = TypeVar("T", bound="Fix")


@_attrs_define
class Fix:
    """An admin's marked break-glass fix (ADR-0089 d3, d8, d15). admin_key_id and admin_person are served on the admin
    routes alone; a person's review read never names the admin. acts is served on GET /v2/review/fixes alone. Since
    v0.8.4.

        Attributes:
            fix_id (UUID):
            act (FixAct):
            target_artifact_id (UUID):
            workspace_id (UUID):
            claim_key (str):
            replaced_artifact_id (None | UUID): The version the fix took out of service.
            served_artifact_id (None | UUID): The version the fix made the claim's served tip, or null.
            reason (str):
            loosens_guardrail (bool):
            state (FixState): superseded: a later fix took over the mark on the version this one served, so no person
                settles it (ADR-0089 d17, the reading of 2026-11-19).
            fixed_at (datetime.datetime):
            age_seconds (int):
            settled_at (datetime.datetime | None):
            same_user (bool | None): Whether the confirmation or revert was by the admin's own person; null while
                unconfirmed.
            restored_artifact_id (None | UUID): The version a revert wrote and promoted.
            superseded_by (None | UUID): The later fix that took over this one's mark, for a superseded fix; null otherwise.
            admin_key_id (UUID | Unset):
            admin_person (str | Unset): The person the admin key acted for when it fixed, recorded then; the settlement
                compares it, never the key's link as it stands (ADR-0089, the operator's decision of 2026-11-19).
            acts (list[FixActsItem] | Unset):
    """

    fix_id: UUID
    act: FixAct
    target_artifact_id: UUID
    workspace_id: UUID
    claim_key: str
    replaced_artifact_id: None | UUID
    served_artifact_id: None | UUID
    reason: str
    loosens_guardrail: bool
    state: FixState
    fixed_at: datetime.datetime
    age_seconds: int
    settled_at: datetime.datetime | None
    same_user: bool | None
    restored_artifact_id: None | UUID
    superseded_by: None | UUID
    admin_key_id: UUID | Unset = UNSET
    admin_person: str | Unset = UNSET
    acts: list[FixActsItem] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        fix_id = str(self.fix_id)

        act = self.act.value

        target_artifact_id = str(self.target_artifact_id)

        workspace_id = str(self.workspace_id)

        claim_key = self.claim_key

        replaced_artifact_id: None | str
        if isinstance(self.replaced_artifact_id, UUID):
            replaced_artifact_id = str(self.replaced_artifact_id)
        else:
            replaced_artifact_id = self.replaced_artifact_id

        served_artifact_id: None | str
        if isinstance(self.served_artifact_id, UUID):
            served_artifact_id = str(self.served_artifact_id)
        else:
            served_artifact_id = self.served_artifact_id

        reason = self.reason

        loosens_guardrail = self.loosens_guardrail

        state = self.state.value

        fixed_at = self.fixed_at.isoformat()

        age_seconds = self.age_seconds

        settled_at: None | str
        if isinstance(self.settled_at, datetime.datetime):
            settled_at = self.settled_at.isoformat()
        else:
            settled_at = self.settled_at

        same_user: bool | None
        same_user = self.same_user

        restored_artifact_id: None | str
        if isinstance(self.restored_artifact_id, UUID):
            restored_artifact_id = str(self.restored_artifact_id)
        else:
            restored_artifact_id = self.restored_artifact_id

        superseded_by: None | str
        if isinstance(self.superseded_by, UUID):
            superseded_by = str(self.superseded_by)
        else:
            superseded_by = self.superseded_by

        admin_key_id: str | Unset = UNSET
        if not isinstance(self.admin_key_id, Unset):
            admin_key_id = str(self.admin_key_id)

        admin_person = self.admin_person

        acts: list[str] | Unset = UNSET
        if not isinstance(self.acts, Unset):
            acts = []
            for acts_item_data in self.acts:
                acts_item = acts_item_data.value
                acts.append(acts_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "fix_id": fix_id,
                "act": act,
                "target_artifact_id": target_artifact_id,
                "workspace_id": workspace_id,
                "claim_key": claim_key,
                "replaced_artifact_id": replaced_artifact_id,
                "served_artifact_id": served_artifact_id,
                "reason": reason,
                "loosens_guardrail": loosens_guardrail,
                "state": state,
                "fixed_at": fixed_at,
                "age_seconds": age_seconds,
                "settled_at": settled_at,
                "same_user": same_user,
                "restored_artifact_id": restored_artifact_id,
                "superseded_by": superseded_by,
            }
        )
        if admin_key_id is not UNSET:
            field_dict["admin_key_id"] = admin_key_id
        if admin_person is not UNSET:
            field_dict["admin_person"] = admin_person
        if acts is not UNSET:
            field_dict["acts"] = acts

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        fix_id = UUID(d.pop("fix_id"))

        act = FixAct(d.pop("act"))

        target_artifact_id = UUID(d.pop("target_artifact_id"))

        workspace_id = UUID(d.pop("workspace_id"))

        claim_key = d.pop("claim_key")

        def _parse_replaced_artifact_id(data: object) -> None | UUID:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                replaced_artifact_id_type_0 = UUID(data)

                return replaced_artifact_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | UUID, data)

        replaced_artifact_id = _parse_replaced_artifact_id(
            d.pop("replaced_artifact_id")
        )

        def _parse_served_artifact_id(data: object) -> None | UUID:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                served_artifact_id_type_0 = UUID(data)

                return served_artifact_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | UUID, data)

        served_artifact_id = _parse_served_artifact_id(d.pop("served_artifact_id"))

        reason = d.pop("reason")

        loosens_guardrail = d.pop("loosens_guardrail")

        state = FixState(d.pop("state"))

        fixed_at = datetime.datetime.fromisoformat(d.pop("fixed_at"))

        age_seconds = d.pop("age_seconds")

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

        def _parse_same_user(data: object) -> bool | None:
            if data is None:
                return data
            return cast(bool | None, data)

        same_user = _parse_same_user(d.pop("same_user"))

        def _parse_restored_artifact_id(data: object) -> None | UUID:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                restored_artifact_id_type_0 = UUID(data)

                return restored_artifact_id_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | UUID, data)

        restored_artifact_id = _parse_restored_artifact_id(
            d.pop("restored_artifact_id")
        )

        def _parse_superseded_by(data: object) -> None | UUID:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                superseded_by_type_0 = UUID(data)

                return superseded_by_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | UUID, data)

        superseded_by = _parse_superseded_by(d.pop("superseded_by"))

        _admin_key_id = d.pop("admin_key_id", UNSET)
        admin_key_id: UUID | Unset
        if isinstance(_admin_key_id, Unset):
            admin_key_id = UNSET
        else:
            admin_key_id = UUID(_admin_key_id)

        admin_person = d.pop("admin_person", UNSET)

        _acts = d.pop("acts", UNSET)
        acts: list[FixActsItem] | Unset = UNSET
        if _acts is not UNSET:
            acts = []
            for acts_item_data in _acts:
                acts_item = FixActsItem(acts_item_data)

                acts.append(acts_item)

        fix = cls(
            fix_id=fix_id,
            act=act,
            target_artifact_id=target_artifact_id,
            workspace_id=workspace_id,
            claim_key=claim_key,
            replaced_artifact_id=replaced_artifact_id,
            served_artifact_id=served_artifact_id,
            reason=reason,
            loosens_guardrail=loosens_guardrail,
            state=state,
            fixed_at=fixed_at,
            age_seconds=age_seconds,
            settled_at=settled_at,
            same_user=same_user,
            restored_artifact_id=restored_artifact_id,
            superseded_by=superseded_by,
            admin_key_id=admin_key_id,
            admin_person=admin_person,
            acts=acts,
        )

        fix.additional_properties = d
        return fix

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
