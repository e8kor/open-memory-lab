from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.workspace_offers_mode import WorkspaceOffersMode

if TYPE_CHECKING:
    from ..models.workspace_offers_commit import WorkspaceOffersCommit
    from ..models.workspace_offers_mode_change_type_0 import (
        WorkspaceOffersModeChangeType0,
    )
    from ..models.workspace_offers_refused_item import WorkspaceOffersRefusedItem
    from ..models.workspace_offers_transitions_item import (
        WorkspaceOffersTransitionsItem,
    )


T = TypeVar("T", bound="WorkspaceOffers")


@_attrs_define
class WorkspaceOffers:
    """
    Attributes:
        workspace_id (UUID):
        status (str):
        mode (WorkspaceOffersMode):
        transitions (list[WorkspaceOffersTransitionsItem]):
        mode_change (None | WorkspaceOffersModeChangeType0): Putting the workspace in progress, where offered; null
            otherwise.
        commit (WorkspaceOffersCommit):
        refused (list[WorkspaceOffersRefusedItem]): Each act of propose protected, propose frozen, mode in_progress and
            mode standard that is not offered, with the refusal its verb would answer now (ADR-0045 d28). Since v0.6.4.
    """

    workspace_id: UUID
    status: str
    mode: WorkspaceOffersMode
    transitions: list[WorkspaceOffersTransitionsItem]
    mode_change: None | WorkspaceOffersModeChangeType0
    commit: WorkspaceOffersCommit
    refused: list[WorkspaceOffersRefusedItem]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.workspace_offers_mode_change_type_0 import (
            WorkspaceOffersModeChangeType0,
        )

        workspace_id = str(self.workspace_id)

        status = self.status

        mode = self.mode.value

        transitions = []
        for transitions_item_data in self.transitions:
            transitions_item = transitions_item_data.to_dict()
            transitions.append(transitions_item)

        mode_change: dict[str, Any] | None
        if isinstance(self.mode_change, WorkspaceOffersModeChangeType0):
            mode_change = self.mode_change.to_dict()
        else:
            mode_change = self.mode_change

        commit = self.commit.to_dict()

        refused = []
        for refused_item_data in self.refused:
            refused_item = refused_item_data.to_dict()
            refused.append(refused_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "workspace_id": workspace_id,
                "status": status,
                "mode": mode,
                "transitions": transitions,
                "mode_change": mode_change,
                "commit": commit,
                "refused": refused,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.workspace_offers_commit import WorkspaceOffersCommit
        from ..models.workspace_offers_mode_change_type_0 import (
            WorkspaceOffersModeChangeType0,
        )
        from ..models.workspace_offers_refused_item import WorkspaceOffersRefusedItem
        from ..models.workspace_offers_transitions_item import (
            WorkspaceOffersTransitionsItem,
        )

        d = dict(src_dict)
        workspace_id = UUID(d.pop("workspace_id"))

        status = d.pop("status")

        mode = WorkspaceOffersMode(d.pop("mode"))

        transitions = []
        _transitions = d.pop("transitions")
        for transitions_item_data in _transitions:
            transitions_item = WorkspaceOffersTransitionsItem.from_dict(
                transitions_item_data
            )

            transitions.append(transitions_item)

        def _parse_mode_change(data: object) -> None | WorkspaceOffersModeChangeType0:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                mode_change_type_0 = WorkspaceOffersModeChangeType0.from_dict(data)

                return mode_change_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | WorkspaceOffersModeChangeType0, data)

        mode_change = _parse_mode_change(d.pop("mode_change"))

        commit = WorkspaceOffersCommit.from_dict(d.pop("commit"))

        refused = []
        _refused = d.pop("refused")
        for refused_item_data in _refused:
            refused_item = WorkspaceOffersRefusedItem.from_dict(refused_item_data)

            refused.append(refused_item)

        workspace_offers = cls(
            workspace_id=workspace_id,
            status=status,
            mode=mode,
            transitions=transitions,
            mode_change=mode_change,
            commit=commit,
            refused=refused,
        )

        workspace_offers.additional_properties = d
        return workspace_offers

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
