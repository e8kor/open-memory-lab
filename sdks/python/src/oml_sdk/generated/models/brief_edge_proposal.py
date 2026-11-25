from __future__ import annotations

from collections.abc import Mapping
from typing import (
    Any,
    Literal,
    Self,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="BriefEdgeProposal")


@_attrs_define
class BriefEdgeProposal:
    """A brief member proposed by a contributor key holding submitter or a reviewer key holding reviewer on the workspace
    (ADR-0095 d6); a claim already proposed for this brief or already a member is 409 proposal.pending, naming the edge
    in details.edge_id.

        Attributes:
            kind (Literal['brief']):
            from_artifact_id (str):
            to_workspace_id (str): A workspace, by its node id, its name or its qualified name. Resolved server-side before
                any check that reads state; an unknown name answers exactly what an unknown id answers. A user key resolves a
                bare name in its own user root's namespace, plus universe and its root's name; every other key in the shared
                namespace (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is universe,
                is neither, and is refused (request.validation).
            reason (str | Unset): Why, scanned as a verdict's reason is and journalled with brief_edge.proposed (ADR-0095
                d1). Since v0.8.5.
    """

    kind: Literal["brief"]
    from_artifact_id: str
    to_workspace_id: str
    reason: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind

        from_artifact_id = self.from_artifact_id

        to_workspace_id: str
        to_workspace_id = self.to_workspace_id

        reason = self.reason

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "kind": kind,
                "from_artifact_id": from_artifact_id,
                "to_workspace_id": to_workspace_id,
            }
        )
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        kind = cast(Literal["brief"], d.pop("kind"))
        if kind != "brief":
            raise ValueError(f"kind must match const 'brief', got '{kind}'")

        from_artifact_id = d.pop("from_artifact_id")

        def _parse_to_workspace_id(data: object) -> str:
            return cast(str, data)

        to_workspace_id = _parse_to_workspace_id(d.pop("to_workspace_id"))

        reason = d.pop("reason", UNSET)

        brief_edge_proposal = cls(
            kind=kind,
            from_artifact_id=from_artifact_id,
            to_workspace_id=to_workspace_id,
            reason=reason,
        )

        return brief_edge_proposal
