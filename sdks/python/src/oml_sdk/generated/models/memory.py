from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.memory_summary_kind import MemorySummaryKind
from ..models.memory_summary_provenance import MemorySummaryProvenance
from ..models.memory_summary_state import MemorySummaryState
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.attachment_descriptor import AttachmentDescriptor
    from ..models.memory_summary_replaces_type_0 import MemorySummaryReplacesType0


T = TypeVar("T", bound="Memory")


@_attrs_define
class Memory:
    """
    Attributes:
        artifact_id (str):
        claim_key (str):
        version (int):
        tip (bool): Whether this version is the served tip of its claim for the calling key, audience-scoped: for a key
            in a simulation's audience an overlay tip that shadows the default tip, which then reads tip false; for every
            other key the default tip (docs/architecture/audit-and-lineage.md). A superseded version is state promoted, tip
            false.
        workarea_id (None | str): Null when the calling key holds no role on the node (naming a node,
            docs/architecture/roles-and-keys.md, Own) — possible only on an own row an admin moved there (ADR-0025 d3).
        workarea_path (None | str): Derived at read time; grants bind to node ids. Null wherever workarea_id is.
        kind (MemorySummaryKind):
        state (MemorySummaryState):
        tokens (int): Budget tokens of the body (counter oml-tc/1): ceil(UTF-8 bytes of the NFC body / 4); not any
            model's count (docs/architecture/retrieval.md, Token budgets).
        retirement (bool): True on a retirement version; a claim whose served tip is one is a retired claim, which
            recall does not serve. Default: False.
        replaces (MemorySummaryReplacesType0 | None): For a supersession, the version it replaces; null for a version 1.
            It obeys the same readability test as derives_from on every read, so it is also null when the caller cannot read
            the replaced version — after an admin moved that version into another workarea or back into held, say (ADR-0025
            d3).
        held_expires_at (datetime.datetime | None): For a held row, when held-item expiry dead-letters it under the
            workarea's review policy; null on every other row, and on every row while held-item expiry is unbuilt (ADR-0025
            d13).
        created_at (datetime.datetime):
        promoted_at (datetime.datetime | None):
        principal (str | Unset):
        provenance (MemorySummaryProvenance | Unset):
        simulation_id (None | str | Unset):
        self_signed (bool | Unset): Marked rows under the bounded development exception; never promote beyond their
            workarea.
        plaintext_digest (str | Unset): SHA-256 of plaintext unless a field says ciphertext.
        derives_from (list[str] | Unset): On get, recall and every listing, only the ancestors the caller can read — its
            own rows in any state, promoted rows within its reader effective access (ADR-0025 d3); the lineage read shows
            the full closure, elided where unreadable.
        attachments (list[AttachmentDescriptor] | Unset):
        body (str | Unset): Decrypted on the serving path only.
    """

    artifact_id: str
    claim_key: str
    version: int
    tip: bool
    workarea_id: None | str
    workarea_path: None | str
    kind: MemorySummaryKind
    state: MemorySummaryState
    tokens: int
    replaces: MemorySummaryReplacesType0 | None
    held_expires_at: datetime.datetime | None
    created_at: datetime.datetime
    promoted_at: datetime.datetime | None
    retirement: bool = False
    principal: str | Unset = UNSET
    provenance: MemorySummaryProvenance | Unset = UNSET
    simulation_id: None | str | Unset = UNSET
    self_signed: bool | Unset = UNSET
    plaintext_digest: str | Unset = UNSET
    derives_from: list[str] | Unset = UNSET
    attachments: list[AttachmentDescriptor] | Unset = UNSET
    body: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.memory_summary_replaces_type_0 import MemorySummaryReplacesType0

        artifact_id = self.artifact_id

        claim_key = self.claim_key

        version = self.version

        tip = self.tip

        workarea_id: None | str
        workarea_id = self.workarea_id

        workarea_path: None | str
        workarea_path = self.workarea_path

        kind = self.kind.value

        state = self.state.value

        tokens = self.tokens

        retirement = self.retirement

        replaces: dict[str, Any] | None
        if isinstance(self.replaces, MemorySummaryReplacesType0):
            replaces = self.replaces.to_dict()
        else:
            replaces = self.replaces

        held_expires_at: None | str
        if isinstance(self.held_expires_at, datetime.datetime):
            held_expires_at = self.held_expires_at.isoformat()
        else:
            held_expires_at = self.held_expires_at

        created_at = self.created_at.isoformat()

        promoted_at: None | str
        if isinstance(self.promoted_at, datetime.datetime):
            promoted_at = self.promoted_at.isoformat()
        else:
            promoted_at = self.promoted_at

        principal = self.principal

        provenance: str | Unset = UNSET
        if not isinstance(self.provenance, Unset):
            provenance = self.provenance.value

        simulation_id: None | str | Unset
        if isinstance(self.simulation_id, Unset):
            simulation_id = UNSET
        else:
            simulation_id = self.simulation_id

        self_signed = self.self_signed

        plaintext_digest = self.plaintext_digest

        derives_from: list[str] | Unset = UNSET
        if not isinstance(self.derives_from, Unset):
            derives_from = self.derives_from

        attachments: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.attachments, Unset):
            attachments = []
            for attachments_item_data in self.attachments:
                attachments_item = attachments_item_data.to_dict()
                attachments.append(attachments_item)

        body = self.body

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "artifact_id": artifact_id,
                "claim_key": claim_key,
                "version": version,
                "tip": tip,
                "workarea_id": workarea_id,
                "workarea_path": workarea_path,
                "kind": kind,
                "state": state,
                "tokens": tokens,
                "retirement": retirement,
                "replaces": replaces,
                "held_expires_at": held_expires_at,
                "created_at": created_at,
                "promoted_at": promoted_at,
            }
        )
        if principal is not UNSET:
            field_dict["principal"] = principal
        if provenance is not UNSET:
            field_dict["provenance"] = provenance
        if simulation_id is not UNSET:
            field_dict["simulation_id"] = simulation_id
        if self_signed is not UNSET:
            field_dict["self_signed"] = self_signed
        if plaintext_digest is not UNSET:
            field_dict["plaintext_digest"] = plaintext_digest
        if derives_from is not UNSET:
            field_dict["derives_from"] = derives_from
        if attachments is not UNSET:
            field_dict["attachments"] = attachments
        if body is not UNSET:
            field_dict["body"] = body

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.attachment_descriptor import AttachmentDescriptor
        from ..models.memory_summary_replaces_type_0 import MemorySummaryReplacesType0

        d = dict(src_dict)
        artifact_id = d.pop("artifact_id")

        claim_key = d.pop("claim_key")

        version = d.pop("version")

        tip = d.pop("tip")

        def _parse_workarea_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        workarea_id = _parse_workarea_id(d.pop("workarea_id"))

        def _parse_workarea_path(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        workarea_path = _parse_workarea_path(d.pop("workarea_path"))

        kind = MemorySummaryKind(d.pop("kind"))

        state = MemorySummaryState(d.pop("state"))

        tokens = d.pop("tokens")

        retirement = d.pop("retirement")

        def _parse_replaces(data: object) -> MemorySummaryReplacesType0 | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                replaces_type_0 = MemorySummaryReplacesType0.from_dict(data)

                return replaces_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(MemorySummaryReplacesType0 | None, data)

        replaces = _parse_replaces(d.pop("replaces"))

        def _parse_held_expires_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                held_expires_at_type_0 = datetime.datetime.fromisoformat(data)

                return held_expires_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        held_expires_at = _parse_held_expires_at(d.pop("held_expires_at"))

        created_at = datetime.datetime.fromisoformat(d.pop("created_at"))

        def _parse_promoted_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                promoted_at_type_0 = datetime.datetime.fromisoformat(data)

                return promoted_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        promoted_at = _parse_promoted_at(d.pop("promoted_at"))

        principal = d.pop("principal", UNSET)

        _provenance = d.pop("provenance", UNSET)
        provenance: MemorySummaryProvenance | Unset
        if isinstance(_provenance, Unset):
            provenance = UNSET
        else:
            provenance = MemorySummaryProvenance(_provenance)

        def _parse_simulation_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        simulation_id = _parse_simulation_id(d.pop("simulation_id", UNSET))

        self_signed = d.pop("self_signed", UNSET)

        plaintext_digest = d.pop("plaintext_digest", UNSET)

        derives_from = cast(list[str], d.pop("derives_from", UNSET))

        _attachments = d.pop("attachments", UNSET)
        attachments: list[AttachmentDescriptor] | Unset = UNSET
        if _attachments is not UNSET:
            attachments = []
            for attachments_item_data in _attachments:
                attachments_item = AttachmentDescriptor.from_dict(attachments_item_data)

                attachments.append(attachments_item)

        body = d.pop("body", UNSET)

        memory = cls(
            artifact_id=artifact_id,
            claim_key=claim_key,
            version=version,
            tip=tip,
            workarea_id=workarea_id,
            workarea_path=workarea_path,
            kind=kind,
            state=state,
            tokens=tokens,
            retirement=retirement,
            replaces=replaces,
            held_expires_at=held_expires_at,
            created_at=created_at,
            promoted_at=promoted_at,
            principal=principal,
            provenance=provenance,
            simulation_id=simulation_id,
            self_signed=self_signed,
            plaintext_digest=plaintext_digest,
            derives_from=derives_from,
            attachments=attachments,
            body=body,
        )

        memory.additional_properties = d
        return memory

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
