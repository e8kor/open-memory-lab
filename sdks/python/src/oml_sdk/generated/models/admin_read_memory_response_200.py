from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.admin_memory_kind import AdminMemoryKind
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
        kind (AdminMemoryKind | Unset): The kind the version was written as; the admin page refuses to supersede or
            retire a guardrail by it (ADR-0037 d17; ADR-0070 d5). Since v0.7.0.
        key_id (str | Unset): The authoring key's id, which names a key that has no name (ADR-0070 d5). Since v0.7.0.
        principal (None | str | Unset):
        user (None | str | Unset):
        format_ (str | Unset): The media type signed with the version (ADR-0056 d1).
        title (None | str | Unset): The title signed with the version, or null (ADR-0056 d10).
        held_by (None | str | Unset): The authoring key's holder, agent, human or service (ADR-0045 d32; ADR-0070 d5).
            Since v0.7.0.
        promoted_at (datetime.datetime | None | Unset): When the version was promoted, or null. Since v0.7.0.
        valid_from (None | str | Unset): When the version says it begins to hold, as signed, or null (ADR-0059 d2).
            Since v0.7.0.
        valid_until (None | str | Unset): When it stops holding, or null when open (ADR-0059 d2). Since v0.7.0.
        scan_finding (None | str | Unset): Admission's scan finding, class: rule at place, never the matched text, or
            null (ADR-0065 d8). Since v0.7.0.
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
    kind: AdminMemoryKind | Unset = UNSET
    key_id: str | Unset = UNSET
    principal: None | str | Unset = UNSET
    user: None | str | Unset = UNSET
    format_: str | Unset = UNSET
    title: None | str | Unset = UNSET
    held_by: None | str | Unset = UNSET
    promoted_at: datetime.datetime | None | Unset = UNSET
    valid_from: None | str | Unset = UNSET
    valid_until: None | str | Unset = UNSET
    scan_finding: None | str | Unset = UNSET
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

        kind: str | Unset = UNSET
        if not isinstance(self.kind, Unset):
            kind = self.kind.value

        key_id = self.key_id

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

        format_ = self.format_

        title: None | str | Unset
        if isinstance(self.title, Unset):
            title = UNSET
        else:
            title = self.title

        held_by: None | str | Unset
        if isinstance(self.held_by, Unset):
            held_by = UNSET
        else:
            held_by = self.held_by

        promoted_at: None | str | Unset
        if isinstance(self.promoted_at, Unset):
            promoted_at = UNSET
        elif isinstance(self.promoted_at, datetime.datetime):
            promoted_at = self.promoted_at.isoformat()
        else:
            promoted_at = self.promoted_at

        valid_from: None | str | Unset
        if isinstance(self.valid_from, Unset):
            valid_from = UNSET
        else:
            valid_from = self.valid_from

        valid_until: None | str | Unset
        if isinstance(self.valid_until, Unset):
            valid_until = UNSET
        else:
            valid_until = self.valid_until

        scan_finding: None | str | Unset
        if isinstance(self.scan_finding, Unset):
            scan_finding = UNSET
        else:
            scan_finding = self.scan_finding

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
        if kind is not UNSET:
            field_dict["kind"] = kind
        if key_id is not UNSET:
            field_dict["key_id"] = key_id
        if principal is not UNSET:
            field_dict["principal"] = principal
        if user is not UNSET:
            field_dict["user"] = user
        if format_ is not UNSET:
            field_dict["format"] = format_
        if title is not UNSET:
            field_dict["title"] = title
        if held_by is not UNSET:
            field_dict["held_by"] = held_by
        if promoted_at is not UNSET:
            field_dict["promoted_at"] = promoted_at
        if valid_from is not UNSET:
            field_dict["valid_from"] = valid_from
        if valid_until is not UNSET:
            field_dict["valid_until"] = valid_until
        if scan_finding is not UNSET:
            field_dict["scan_finding"] = scan_finding
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

        _kind = d.pop("kind", UNSET)
        kind: AdminMemoryKind | Unset
        if isinstance(_kind, Unset):
            kind = UNSET
        else:
            kind = AdminMemoryKind(_kind)

        key_id = d.pop("key_id", UNSET)

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

        format_ = d.pop("format", UNSET)

        def _parse_title(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        title = _parse_title(d.pop("title", UNSET))

        def _parse_held_by(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        held_by = _parse_held_by(d.pop("held_by", UNSET))

        def _parse_promoted_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                promoted_at_type_0 = datetime.datetime.fromisoformat(data)

                return promoted_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        promoted_at = _parse_promoted_at(d.pop("promoted_at", UNSET))

        def _parse_valid_from(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        valid_from = _parse_valid_from(d.pop("valid_from", UNSET))

        def _parse_valid_until(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        valid_until = _parse_valid_until(d.pop("valid_until", UNSET))

        def _parse_scan_finding(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        scan_finding = _parse_scan_finding(d.pop("scan_finding", UNSET))

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
            kind=kind,
            key_id=key_id,
            principal=principal,
            user=user,
            format_=format_,
            title=title,
            held_by=held_by,
            promoted_at=promoted_at,
            valid_from=valid_from,
            valid_until=valid_until,
            scan_finding=scan_finding,
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
