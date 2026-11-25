from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.transition_from import TransitionFrom
from ..models.transition_state import TransitionState
from ..models.transition_to import TransitionTo
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.proposed_by_type_0 import ProposedByType0


T = TypeVar("T", bound="Transition")


@_attrs_define
class Transition:
    """
    Attributes:
        transition_id (str):
        workspace_id (str):
        from_ (TransitionFrom):
        to (TransitionTo):
        state (TransitionState):
        proposed_by_key_id (None | str): The proposing key, served only to that key; null to every other reader, an
            admin key included (ADR-0025 d15).
        ratified_by_key_id (None | str): Null on this build's answers; the journal's workspace.status_transition names
            the ratifier.
        proposed_at (datetime.datetime | Unset): The instant of its workspace.status_proposed, to every reader (ADR-0095
            d5 as amended 2026-11-24). Since v0.8.5.
        proposed_by (None | ProposedByType0 | Unset): The key that made a live proposal, served when it is the calling
            key or another key of the same person, and null to every other reader (ADR-0095, amendment of 2026-11-24). Since
            v0.8.5.
    """

    transition_id: str
    workspace_id: str
    from_: TransitionFrom
    to: TransitionTo
    state: TransitionState
    proposed_by_key_id: None | str
    ratified_by_key_id: None | str
    proposed_at: datetime.datetime | Unset = UNSET
    proposed_by: None | ProposedByType0 | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.proposed_by_type_0 import ProposedByType0

        transition_id = self.transition_id

        workspace_id = self.workspace_id

        from_ = self.from_.value

        to = self.to.value

        state = self.state.value

        proposed_by_key_id: None | str
        proposed_by_key_id = self.proposed_by_key_id

        ratified_by_key_id: None | str
        ratified_by_key_id = self.ratified_by_key_id

        proposed_at: str | Unset = UNSET
        if not isinstance(self.proposed_at, Unset):
            proposed_at = self.proposed_at.isoformat()

        proposed_by: dict[str, Any] | None | Unset
        if isinstance(self.proposed_by, Unset):
            proposed_by = UNSET
        elif isinstance(self.proposed_by, ProposedByType0):
            proposed_by = self.proposed_by.to_dict()
        else:
            proposed_by = self.proposed_by

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "transition_id": transition_id,
                "workspace_id": workspace_id,
                "from": from_,
                "to": to,
                "state": state,
                "proposed_by_key_id": proposed_by_key_id,
                "ratified_by_key_id": ratified_by_key_id,
            }
        )
        if proposed_at is not UNSET:
            field_dict["proposed_at"] = proposed_at
        if proposed_by is not UNSET:
            field_dict["proposed_by"] = proposed_by

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.proposed_by_type_0 import ProposedByType0

        d = dict(src_dict)
        transition_id = d.pop("transition_id")

        workspace_id = d.pop("workspace_id")

        from_ = TransitionFrom(d.pop("from"))

        to = TransitionTo(d.pop("to"))

        state = TransitionState(d.pop("state"))

        def _parse_proposed_by_key_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        proposed_by_key_id = _parse_proposed_by_key_id(d.pop("proposed_by_key_id"))

        def _parse_ratified_by_key_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        ratified_by_key_id = _parse_ratified_by_key_id(d.pop("ratified_by_key_id"))

        _proposed_at = d.pop("proposed_at", UNSET)
        proposed_at: datetime.datetime | Unset
        if isinstance(_proposed_at, Unset):
            proposed_at = UNSET
        else:
            proposed_at = datetime.datetime.fromisoformat(_proposed_at)

        def _parse_proposed_by(data: object) -> None | ProposedByType0 | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_proposed_by_type_0 = ProposedByType0.from_dict(data)

                return componentsschemas_proposed_by_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ProposedByType0 | Unset, data)

        proposed_by = _parse_proposed_by(d.pop("proposed_by", UNSET))

        transition = cls(
            transition_id=transition_id,
            workspace_id=workspace_id,
            from_=from_,
            to=to,
            state=state,
            proposed_by_key_id=proposed_by_key_id,
            ratified_by_key_id=ratified_by_key_id,
            proposed_at=proposed_at,
            proposed_by=proposed_by,
        )

        transition.additional_properties = d
        return transition

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
