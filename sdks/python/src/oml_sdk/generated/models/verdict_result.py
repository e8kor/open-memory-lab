from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.verdict_result_state import VerdictResultState
from ..models.verdict_result_verdict import VerdictResultVerdict
from ..types import UNSET, Unset

T = TypeVar("T", bound="VerdictResult")


@_attrs_define
class VerdictResult:
    """
    Attributes:
        artifact_id (str):
        version (int):
        verdict (VerdictResultVerdict):
        state (VerdictResultState): held means the policy's k needs more verdicts.
        ratification_event_id (None | str | Unset):
    """

    artifact_id: str
    version: int
    verdict: VerdictResultVerdict
    state: VerdictResultState
    ratification_event_id: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        artifact_id = self.artifact_id

        version = self.version

        verdict = self.verdict.value

        state = self.state.value

        ratification_event_id: None | str | Unset
        if isinstance(self.ratification_event_id, Unset):
            ratification_event_id = UNSET
        else:
            ratification_event_id = self.ratification_event_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "artifact_id": artifact_id,
                "version": version,
                "verdict": verdict,
                "state": state,
            }
        )
        if ratification_event_id is not UNSET:
            field_dict["ratification_event_id"] = ratification_event_id

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        artifact_id = d.pop("artifact_id")

        version = d.pop("version")

        verdict = VerdictResultVerdict(d.pop("verdict"))

        state = VerdictResultState(d.pop("state"))

        def _parse_ratification_event_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        ratification_event_id = _parse_ratification_event_id(
            d.pop("ratification_event_id", UNSET)
        )

        verdict_result = cls(
            artifact_id=artifact_id,
            version=version,
            verdict=verdict,
            state=state,
            ratification_event_id=ratification_event_id,
        )

        verdict_result.additional_properties = d
        return verdict_result

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
