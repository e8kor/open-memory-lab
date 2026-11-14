from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="SetWorkareaFederationResponse200")


@_attrs_define
class SetWorkareaFederationResponse200:
    """
    Attributes:
        accept_federated (bool):
    """

    accept_federated: bool

    def to_dict(self) -> dict[str, Any]:
        accept_federated = self.accept_federated

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "accept_federated": accept_federated,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        accept_federated = d.pop("accept_federated")

        set_workarea_federation_response_200 = cls(
            accept_federated=accept_federated,
        )

        return set_workarea_federation_response_200
