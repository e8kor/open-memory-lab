from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.admin_memory_state import AdminMemoryState
from ..types import UNSET, Unset

T = TypeVar("T", bound="AdminMemory")


@_attrs_define
class AdminMemory:
    """A memory as the admin read serves it (ADR-0045 d21): its text and who wrote it, in every state.

    Attributes:
        artifact_id (str):
        claim_key (str):
        version (int):
        state (AdminMemoryState):
        workarea_path (str):
        text (str):
        key (str): The authoring key's name.
        created_at (datetime.datetime):
        principal (None | str | Unset):
        user (None | str | Unset):
    """

    artifact_id: str
    claim_key: str
    version: int
    state: AdminMemoryState
    workarea_path: str
    text: str
    key: str
    created_at: datetime.datetime
    principal: None | str | Unset = UNSET
    user: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        artifact_id = self.artifact_id

        claim_key = self.claim_key

        version = self.version

        state = self.state.value

        workarea_path = self.workarea_path

        text = self.text

        key = self.key

        created_at = self.created_at.isoformat()

        principal: None | str | Unset
        if isinstance(self.principal, Unset):
            principal = UNSET
        else:
            principal = self.principal

        user: None | str | Unset
        if isinstance(self.user, Unset):
            user = UNSET
        else:
            user = self.user

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "artifact_id": artifact_id,
                "claim_key": claim_key,
                "version": version,
                "state": state,
                "workarea_path": workarea_path,
                "text": text,
                "key": key,
                "created_at": created_at,
            }
        )
        if principal is not UNSET:
            field_dict["principal"] = principal
        if user is not UNSET:
            field_dict["user"] = user

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        artifact_id = d.pop("artifact_id")

        claim_key = d.pop("claim_key")

        version = d.pop("version")

        state = AdminMemoryState(d.pop("state"))

        workarea_path = d.pop("workarea_path")

        text = d.pop("text")

        key = d.pop("key")

        created_at = datetime.datetime.fromisoformat(d.pop("created_at"))

        def _parse_principal(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        principal = _parse_principal(d.pop("principal", UNSET))

        def _parse_user(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        user = _parse_user(d.pop("user", UNSET))

        admin_memory = cls(
            artifact_id=artifact_id,
            claim_key=claim_key,
            version=version,
            state=state,
            workarea_path=workarea_path,
            text=text,
            key=key,
            created_at=created_at,
            principal=principal,
            user=user,
        )

        admin_memory.additional_properties = d
        return admin_memory

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
