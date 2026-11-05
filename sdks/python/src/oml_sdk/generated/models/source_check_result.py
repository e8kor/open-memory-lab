from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.stale import Stale

if TYPE_CHECKING:
    from ..models.source_checked import SourceChecked


T = TypeVar("T", bound="SourceCheckResult")


@_attrs_define
class SourceCheckResult:
    """
    Attributes:
        artifact_id (str):
        stale (None | Stale):
        source_checked (SourceChecked):
        journal_seq (int):
    """

    artifact_id: str
    stale: None | Stale
    source_checked: SourceChecked
    journal_seq: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        artifact_id = self.artifact_id

        stale: None | str
        if isinstance(self.stale, Stale):
            stale = self.stale.value
        else:
            stale = self.stale

        source_checked = self.source_checked.to_dict()

        journal_seq = self.journal_seq

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "artifact_id": artifact_id,
                "stale": stale,
                "source_checked": source_checked,
                "journal_seq": journal_seq,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.source_checked import SourceChecked

        d = dict(src_dict)
        artifact_id = d.pop("artifact_id")

        def _parse_stale(data: object) -> None | Stale:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                stale_type_0 = Stale(data)

                return stale_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Stale, data)

        stale = _parse_stale(d.pop("stale"))

        source_checked = SourceChecked.from_dict(d.pop("source_checked"))

        journal_seq = d.pop("journal_seq")

        source_check_result = cls(
            artifact_id=artifact_id,
            stale=stale,
            source_checked=source_checked,
            journal_seq=journal_seq,
        )

        source_check_result.additional_properties = d
        return source_check_result

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
