from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.verdict_request_verdict import VerdictRequestVerdict
from ..types import UNSET, Unset

T = TypeVar("T", bound="VerdictRequest")


@_attrs_define
class VerdictRequest:
    """
    Attributes:
        verdict (VerdictRequestVerdict):
        serve_id (str | Unset): The serve review next answered. Omitted, the one item this key has claimed; refused
            naming serve_id when it holds none or several (ADR-0039 d10).
        reason (str | Unset): Required to reject.
    """

    verdict: VerdictRequestVerdict
    serve_id: str | Unset = UNSET
    reason: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        verdict = self.verdict.value

        serve_id = self.serve_id

        reason = self.reason

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "verdict": verdict,
            }
        )
        if serve_id is not UNSET:
            field_dict["serve_id"] = serve_id
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        verdict = VerdictRequestVerdict(d.pop("verdict"))

        serve_id = d.pop("serve_id", UNSET)

        reason = d.pop("reason", UNSET)

        verdict_request = cls(
            verdict=verdict,
            serve_id=serve_id,
            reason=reason,
        )

        verdict_request.additional_properties = d
        return verdict_request

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
