from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.journal_event_payload import JournalEventPayload


T = TypeVar("T", bound="JournalEvent")


@_attrs_define
class JournalEvent:
    """The event-catalog envelope; payloads are digest-only, never body plaintext (D1).

    Attributes:
        seq (int):
        event_id (str):
        event_type (str):
        at (datetime.datetime):
        prev_hash (str | Unset):
        hash_ (str | Unset):
        sig (str | Unset):
        principal (None | str | Unset): Null on the events tail for a key outside the actor's own key set
            (docs/api/http-api.md, Events tail).
        api_key_id (None | str | Unset): The actor's key; null on the events tail for a key outside the actor's own key
            set, as is every payload field naming a key.
        payload (JournalEventPayload | Unset):
    """

    seq: int
    event_id: str
    event_type: str
    at: datetime.datetime
    prev_hash: str | Unset = UNSET
    hash_: str | Unset = UNSET
    sig: str | Unset = UNSET
    principal: None | str | Unset = UNSET
    api_key_id: None | str | Unset = UNSET
    payload: JournalEventPayload | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        seq = self.seq

        event_id = self.event_id

        event_type = self.event_type

        at = self.at.isoformat()

        prev_hash = self.prev_hash

        hash_ = self.hash_

        sig = self.sig

        principal: None | str | Unset
        if isinstance(self.principal, Unset):
            principal = UNSET
        else:
            principal = self.principal

        api_key_id: None | str | Unset
        if isinstance(self.api_key_id, Unset):
            api_key_id = UNSET
        else:
            api_key_id = self.api_key_id

        payload: dict[str, Any] | Unset = UNSET
        if not isinstance(self.payload, Unset):
            payload = self.payload.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "seq": seq,
                "event_id": event_id,
                "event_type": event_type,
                "at": at,
            }
        )
        if prev_hash is not UNSET:
            field_dict["prev_hash"] = prev_hash
        if hash_ is not UNSET:
            field_dict["hash"] = hash_
        if sig is not UNSET:
            field_dict["sig"] = sig
        if principal is not UNSET:
            field_dict["principal"] = principal
        if api_key_id is not UNSET:
            field_dict["api_key_id"] = api_key_id
        if payload is not UNSET:
            field_dict["payload"] = payload

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.journal_event_payload import JournalEventPayload

        d = dict(src_dict)
        seq = d.pop("seq")

        event_id = d.pop("event_id")

        event_type = d.pop("event_type")

        at = datetime.datetime.fromisoformat(d.pop("at"))

        prev_hash = d.pop("prev_hash", UNSET)

        hash_ = d.pop("hash", UNSET)

        sig = d.pop("sig", UNSET)

        def _parse_principal(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        principal = _parse_principal(d.pop("principal", UNSET))

        def _parse_api_key_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        api_key_id = _parse_api_key_id(d.pop("api_key_id", UNSET))

        _payload = d.pop("payload", UNSET)
        payload: JournalEventPayload | Unset
        if isinstance(_payload, Unset):
            payload = UNSET
        else:
            payload = JournalEventPayload.from_dict(_payload)

        journal_event = cls(
            seq=seq,
            event_id=event_id,
            event_type=event_type,
            at=at,
            prev_hash=prev_hash,
            hash_=hash_,
            sig=sig,
            principal=principal,
            api_key_id=api_key_id,
            payload=payload,
        )

        journal_event.additional_properties = d
        return journal_event

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
