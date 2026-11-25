from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.transition_proposal_to import TransitionProposalTo
from ..types import UNSET, Unset

T = TypeVar("T", bound="TransitionProposal")


@_attrs_define
class TransitionProposal:
    """
    Attributes:
        to (TransitionProposalTo): Forward only; backward transitions do not exist, and a user root and every workspace
            under it stay development, so a proposal there is refused workspace.transition_refused, reason user_root
            (ADR-0085 d17).
        reason (str | Unset): Why, scanned as a verdict's reason is and journalled with workspace.status_proposed
            (ADR-0095 d1). Since v0.8.5.
    """

    to: TransitionProposalTo
    reason: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        to = self.to.value

        reason = self.reason

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "to": to,
            }
        )
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        to = TransitionProposalTo(d.pop("to"))

        reason = d.pop("reason", UNSET)

        transition_proposal = cls(
            to=to,
            reason=reason,
        )

        return transition_proposal
