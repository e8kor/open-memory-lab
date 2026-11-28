from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="SetWorkspaceFederationBody")


@_attrs_define
class SetWorkspaceFederationBody:
    """
    Attributes:
        accept_federated (bool):
        reason (str | Unset): An admin act's reason, scanned as a verdict's (ADR-0097 d26): an admin key's true is
            widening, and waits on a second admin person, which requires a reason, unless the tenant has only ever had one
            admin person. Since v0.8.6.
    """

    accept_federated: bool
    reason: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        accept_federated = self.accept_federated

        reason = self.reason

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "accept_federated": accept_federated,
            }
        )
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        accept_federated = d.pop("accept_federated")

        reason = d.pop("reason", UNSET)

        set_workspace_federation_body = cls(
            accept_federated=accept_federated,
            reason=reason,
        )

        return set_workspace_federation_body
