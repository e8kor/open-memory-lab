from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.brief_proposal_proposed_by import BriefProposalProposedBy
    from ..models.brief_proposal_workspace import BriefProposalWorkspace


T = TypeVar("T", bound="BriefProposal")


@_attrs_define
class BriefProposal:
    """One pending brief edge a person's reviewer key may ratify (ADR-0096 d7).

    Attributes:
        edge_id (UUID):
        workspace (BriefProposalWorkspace):
        artifact_id (UUID):
        claim_key (None | str):
        title (None | str):
        proposed_by (BriefProposalProposedBy):
        same_user (bool):
        proposed_at (datetime.datetime):
    """

    edge_id: UUID
    workspace: BriefProposalWorkspace
    artifact_id: UUID
    claim_key: None | str
    title: None | str
    proposed_by: BriefProposalProposedBy
    same_user: bool
    proposed_at: datetime.datetime

    def to_dict(self) -> dict[str, Any]:
        edge_id = str(self.edge_id)

        workspace = self.workspace.to_dict()

        artifact_id = str(self.artifact_id)

        claim_key: None | str
        claim_key = self.claim_key

        title: None | str
        title = self.title

        proposed_by = self.proposed_by.to_dict()

        same_user = self.same_user

        proposed_at = self.proposed_at.isoformat()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "edge_id": edge_id,
                "workspace": workspace,
                "artifact_id": artifact_id,
                "claim_key": claim_key,
                "title": title,
                "proposed_by": proposed_by,
                "same_user": same_user,
                "proposed_at": proposed_at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.brief_proposal_proposed_by import BriefProposalProposedBy
        from ..models.brief_proposal_workspace import BriefProposalWorkspace

        d = dict(src_dict)
        edge_id = UUID(d.pop("edge_id"))

        workspace = BriefProposalWorkspace.from_dict(d.pop("workspace"))

        artifact_id = UUID(d.pop("artifact_id"))

        def _parse_claim_key(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        claim_key = _parse_claim_key(d.pop("claim_key"))

        def _parse_title(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        title = _parse_title(d.pop("title"))

        proposed_by = BriefProposalProposedBy.from_dict(d.pop("proposed_by"))

        same_user = d.pop("same_user")

        proposed_at = datetime.datetime.fromisoformat(d.pop("proposed_at"))

        brief_proposal = cls(
            edge_id=edge_id,
            workspace=workspace,
            artifact_id=artifact_id,
            claim_key=claim_key,
            title=title,
            proposed_by=proposed_by,
            same_user=same_user,
            proposed_at=proposed_at,
        )

        return brief_proposal
