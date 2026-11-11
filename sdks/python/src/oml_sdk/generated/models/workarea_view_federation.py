from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="WorkareaViewFederation")


@_attrs_define
class WorkareaViewFederation:
    """The node's federation field (ADR-0072 d5).

    Attributes:
        accept_federated (bool): Whether recalls scoped elsewhere that ask for wider sources may search this node; true
            by default. It grants nothing.
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

        workarea_view_federation = cls(
            accept_federated=accept_federated,
        )

        return workarea_view_federation
