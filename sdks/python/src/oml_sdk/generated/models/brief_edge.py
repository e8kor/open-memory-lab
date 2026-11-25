from __future__ import annotations

import datetime
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

from ..models.brief_edge_state import BriefEdgeState
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.proposed_by_type_0 import ProposedByType0


T = TypeVar("T", bound="BriefEdge")


@_attrs_define
class BriefEdge:
    """
    Attributes:
        kind (Literal['brief']):
        edge_id (str):
        from_artifact_id (str):
        to_workspace_id (str):
        label (Literal['brief']):
        state (BriefEdgeState):
        proposer_key_id (str):
        ratifier_key_id (None | str):
        proposed_at (datetime.datetime | Unset): On a pending edge the brief's listing serves: the instant of its
            brief_edge.proposed (ADR-0095 d6 as amended 2026-11-24). Since v0.8.5.
        proposed_by (None | ProposedByType0 | Unset): The key that made a live proposal, served when it is the calling
            key or another key of the same person, and null to every other reader (ADR-0095, amendment of 2026-11-24). Since
            v0.8.5.
    """

    kind: Literal["brief"]
    edge_id: str
    from_artifact_id: str
    to_workspace_id: str
    label: Literal["brief"]
    state: BriefEdgeState
    proposer_key_id: str
    ratifier_key_id: None | str
    proposed_at: datetime.datetime | Unset = UNSET
    proposed_by: None | ProposedByType0 | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.proposed_by_type_0 import ProposedByType0

        kind = self.kind

        edge_id = self.edge_id

        from_artifact_id = self.from_artifact_id

        to_workspace_id = self.to_workspace_id

        label = self.label

        state = self.state.value

        proposer_key_id = self.proposer_key_id

        ratifier_key_id: None | str
        ratifier_key_id = self.ratifier_key_id

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
                "kind": kind,
                "edge_id": edge_id,
                "from_artifact_id": from_artifact_id,
                "to_workspace_id": to_workspace_id,
                "label": label,
                "state": state,
                "proposer_key_id": proposer_key_id,
                "ratifier_key_id": ratifier_key_id,
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
        kind = cast(Literal["brief"], d.pop("kind"))
        if kind != "brief":
            raise ValueError(f"kind must match const 'brief', got '{kind}'")

        edge_id = d.pop("edge_id")

        from_artifact_id = d.pop("from_artifact_id")

        to_workspace_id = d.pop("to_workspace_id")

        label = cast(Literal["brief"], d.pop("label"))
        if label != "brief":
            raise ValueError(f"label must match const 'brief', got '{label}'")

        state = BriefEdgeState(d.pop("state"))

        proposer_key_id = d.pop("proposer_key_id")

        def _parse_ratifier_key_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        ratifier_key_id = _parse_ratifier_key_id(d.pop("ratifier_key_id"))

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

        brief_edge = cls(
            kind=kind,
            edge_id=edge_id,
            from_artifact_id=from_artifact_id,
            to_workspace_id=to_workspace_id,
            label=label,
            state=state,
            proposer_key_id=proposer_key_id,
            ratifier_key_id=ratifier_key_id,
            proposed_at=proposed_at,
            proposed_by=proposed_by,
        )

        brief_edge.additional_properties = d
        return brief_edge

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
