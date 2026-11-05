from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.admin_memory_state import AdminMemoryState
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.admin_read_memory_response_200_verdicts_item import (
        AdminReadMemoryResponse200VerdictsItem,
    )
    from ..models.anchor import Anchor


T = TypeVar("T", bound="AdminReadMemoryResponse200")


@_attrs_define
class AdminReadMemoryResponse200:
    """
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
        anchor (Anchor | None | Unset): The version's anchor, which the reviewer judges beside the text, or null
            (ADR-0057 d2).
        derives_from (list[str] | Unset):
        replaces (None | str | Unset):
        plaintext_digest (str | Unset):
        verdicts (list[AdminReadMemoryResponse200VerdictsItem] | Unset):
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
    anchor: Anchor | None | Unset = UNSET
    derives_from: list[str] | Unset = UNSET
    replaces: None | str | Unset = UNSET
    plaintext_digest: str | Unset = UNSET
    verdicts: list[AdminReadMemoryResponse200VerdictsItem] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.anchor import Anchor

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

        anchor: dict[str, Any] | None | Unset
        if isinstance(self.anchor, Unset):
            anchor = UNSET
        elif isinstance(self.anchor, Anchor):
            anchor = self.anchor.to_dict()
        else:
            anchor = self.anchor

        derives_from: list[str] | Unset = UNSET
        if not isinstance(self.derives_from, Unset):
            derives_from = self.derives_from

        replaces: None | str | Unset
        if isinstance(self.replaces, Unset):
            replaces = UNSET
        else:
            replaces = self.replaces

        plaintext_digest = self.plaintext_digest

        verdicts: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.verdicts, Unset):
            verdicts = []
            for verdicts_item_data in self.verdicts:
                verdicts_item = verdicts_item_data.to_dict()
                verdicts.append(verdicts_item)

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
        if anchor is not UNSET:
            field_dict["anchor"] = anchor
        if derives_from is not UNSET:
            field_dict["derives_from"] = derives_from
        if replaces is not UNSET:
            field_dict["replaces"] = replaces
        if plaintext_digest is not UNSET:
            field_dict["plaintext_digest"] = plaintext_digest
        if verdicts is not UNSET:
            field_dict["verdicts"] = verdicts

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.admin_read_memory_response_200_verdicts_item import (
            AdminReadMemoryResponse200VerdictsItem,
        )
        from ..models.anchor import Anchor

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

        def _parse_anchor(data: object) -> Anchor | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                anchor_type_0 = Anchor.from_dict(data)

                return anchor_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(Anchor | None | Unset, data)

        anchor = _parse_anchor(d.pop("anchor", UNSET))

        derives_from = cast(list[str], d.pop("derives_from", UNSET))

        def _parse_replaces(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        replaces = _parse_replaces(d.pop("replaces", UNSET))

        plaintext_digest = d.pop("plaintext_digest", UNSET)

        _verdicts = d.pop("verdicts", UNSET)
        verdicts: list[AdminReadMemoryResponse200VerdictsItem] | Unset = UNSET
        if _verdicts is not UNSET:
            verdicts = []
            for verdicts_item_data in _verdicts:
                verdicts_item = AdminReadMemoryResponse200VerdictsItem.from_dict(
                    verdicts_item_data
                )

                verdicts.append(verdicts_item)

        admin_read_memory_response_200 = cls(
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
            anchor=anchor,
            derives_from=derives_from,
            replaces=replaces,
            plaintext_digest=plaintext_digest,
            verdicts=verdicts,
        )

        admin_read_memory_response_200.additional_properties = d
        return admin_read_memory_response_200

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
