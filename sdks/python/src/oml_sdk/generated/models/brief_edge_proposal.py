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

T = TypeVar("T", bound="BriefEdgeProposal")


@_attrs_define
class BriefEdgeProposal:
    """
    Attributes:
        kind (Literal['brief']):
        from_artifact_id (str):
        to_workarea_id (str): A workarea, by its node id or its name. Resolved server-side before any check that reads
            state; an unknown name answers exactly what an unknown id answers. A derived path is neither, and is refused
            (request.validation).
    """

    kind: Literal["brief"]
    from_artifact_id: str
    to_workarea_id: str

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind

        from_artifact_id = self.from_artifact_id

        to_workarea_id: str
        to_workarea_id = self.to_workarea_id

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "kind": kind,
                "from_artifact_id": from_artifact_id,
                "to_workarea_id": to_workarea_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        kind = cast(Literal["brief"], d.pop("kind"))
        if kind != "brief":
            raise ValueError(f"kind must match const 'brief', got '{kind}'")

        from_artifact_id = d.pop("from_artifact_id")

        def _parse_to_workarea_id(data: object) -> str:
            return cast(str, data)

        to_workarea_id = _parse_to_workarea_id(d.pop("to_workarea_id"))

        brief_edge_proposal = cls(
            kind=kind,
            from_artifact_id=from_artifact_id,
            to_workarea_id=to_workarea_id,
        )

        return brief_edge_proposal
