from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.transition_proposal_to import TransitionProposalTo

T = TypeVar("T", bound="TransitionProposal")


@_attrs_define
class TransitionProposal:
    """
    Attributes:
        to (TransitionProposalTo): Forward only; backward transitions do not exist, and a user root climbs to committed
            at most (ADR-0046 d1).
    """

    to: TransitionProposalTo

    def to_dict(self) -> dict[str, Any]:
        to = self.to.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "to": to,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        to = TransitionProposalTo(d.pop("to"))

        transition_proposal = cls(
            to=to,
        )

        return transition_proposal
