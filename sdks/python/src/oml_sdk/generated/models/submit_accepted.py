from __future__ import annotations

from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    Literal,
    Self,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.submit_accepted_replaces_type_0 import SubmitAcceptedReplacesType0
    from ..models.submit_accepted_scan import SubmitAcceptedScan


T = TypeVar("T", bound="SubmitAccepted")


@_attrs_define
class SubmitAccepted:
    """
    Attributes:
        artifact_id (str):
        claim_key (str):
        version (int): The ordinal within the claim_key; for a successor, 1 + the greatest version of the claim_key in
            any state, so it counts versions the key may not read — a stated residual, FR-31 (docs/architecture/audit-and-
            lineage.md).
        state (Literal['held']):
        workarea_id (str): The workarea the submission was captured in — the resolved target, or a supersession target's
            workarea.
        retirement (bool): True when the submission is a retirement version. Default: False.
        replaces (None | SubmitAcceptedReplacesType0): For a supersession, the version it replaces; null for a version
            1.
        plaintext_digest (str | Unset): SHA-256 of plaintext unless a field says ciphertext.
        scan (SubmitAcceptedScan | Unset):
        journal_seq (int | Unset):
    """

    artifact_id: str
    claim_key: str
    version: int
    state: Literal["held"]
    workarea_id: str
    replaces: None | SubmitAcceptedReplacesType0
    retirement: bool = False
    plaintext_digest: str | Unset = UNSET
    scan: SubmitAcceptedScan | Unset = UNSET
    journal_seq: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.submit_accepted_replaces_type_0 import SubmitAcceptedReplacesType0

        artifact_id = self.artifact_id

        claim_key = self.claim_key

        version = self.version

        state = self.state

        workarea_id = self.workarea_id

        retirement = self.retirement

        replaces: dict[str, Any] | None
        if isinstance(self.replaces, SubmitAcceptedReplacesType0):
            replaces = self.replaces.to_dict()
        else:
            replaces = self.replaces

        plaintext_digest = self.plaintext_digest

        scan: dict[str, Any] | Unset = UNSET
        if not isinstance(self.scan, Unset):
            scan = self.scan.to_dict()

        journal_seq = self.journal_seq

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "artifact_id": artifact_id,
                "claim_key": claim_key,
                "version": version,
                "state": state,
                "workarea_id": workarea_id,
                "retirement": retirement,
                "replaces": replaces,
            }
        )
        if plaintext_digest is not UNSET:
            field_dict["plaintext_digest"] = plaintext_digest
        if scan is not UNSET:
            field_dict["scan"] = scan
        if journal_seq is not UNSET:
            field_dict["journal_seq"] = journal_seq

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.submit_accepted_replaces_type_0 import SubmitAcceptedReplacesType0
        from ..models.submit_accepted_scan import SubmitAcceptedScan

        d = dict(src_dict)
        artifact_id = d.pop("artifact_id")

        claim_key = d.pop("claim_key")

        version = d.pop("version")

        state = cast(Literal["held"], d.pop("state"))
        if state != "held":
            raise ValueError(f"state must match const 'held', got '{state}'")

        workarea_id = d.pop("workarea_id")

        retirement = d.pop("retirement")

        def _parse_replaces(data: object) -> None | SubmitAcceptedReplacesType0:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                replaces_type_0 = SubmitAcceptedReplacesType0.from_dict(data)

                return replaces_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | SubmitAcceptedReplacesType0, data)

        replaces = _parse_replaces(d.pop("replaces"))

        plaintext_digest = d.pop("plaintext_digest", UNSET)

        _scan = d.pop("scan", UNSET)
        scan: SubmitAcceptedScan | Unset
        if isinstance(_scan, Unset):
            scan = UNSET
        else:
            scan = SubmitAcceptedScan.from_dict(_scan)

        journal_seq = d.pop("journal_seq", UNSET)

        submit_accepted = cls(
            artifact_id=artifact_id,
            claim_key=claim_key,
            version=version,
            state=state,
            workarea_id=workarea_id,
            retirement=retirement,
            replaces=replaces,
            plaintext_digest=plaintext_digest,
            scan=scan,
            journal_seq=journal_seq,
        )

        submit_accepted.additional_properties = d
        return submit_accepted

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
