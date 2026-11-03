from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.export_request_kind import ExportRequestKind
from ..types import UNSET, Unset

T = TypeVar("T", bound="ExportRequest")


@_attrs_define
class ExportRequest:
    """
    Attributes:
        kind (ExportRequestKind): Signed evidence bundle, or digest-only chain stream for client-side verification.
        from_seq (int | Unset):
        include_simulations (bool | Unset): When true, simulation rows are listed separately by simulation_id. Default:
            False.
    """

    kind: ExportRequestKind
    from_seq: int | Unset = UNSET
    include_simulations: bool | Unset = False
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind.value

        from_seq = self.from_seq

        include_simulations = self.include_simulations

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "kind": kind,
            }
        )
        if from_seq is not UNSET:
            field_dict["from_seq"] = from_seq
        if include_simulations is not UNSET:
            field_dict["include_simulations"] = include_simulations

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        kind = ExportRequestKind(d.pop("kind"))

        from_seq = d.pop("from_seq", UNSET)

        include_simulations = d.pop("include_simulations", UNSET)

        export_request = cls(
            kind=kind,
            from_seq=from_seq,
            include_simulations=include_simulations,
        )

        export_request.additional_properties = d
        return export_request

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
