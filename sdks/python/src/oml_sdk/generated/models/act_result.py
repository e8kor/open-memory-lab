from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.act_result_preview import ActResultPreview


T = TypeVar("T", bound="ActResult")


@_attrs_define
class ActResult:
    """Outcome of a journalled act, or a dry-run preview when dry_run was true.

    Attributes:
        journal_seq (int | None | Unset): Null for a dry run — nothing was journalled.
        event_id (None | str | Unset):
        dry_run (bool | Unset):
        preview (ActResultPreview | Unset): Reach preview for mass acts (paths rewritten, rows demoted, inventory).
    """

    journal_seq: int | None | Unset = UNSET
    event_id: None | str | Unset = UNSET
    dry_run: bool | Unset = UNSET
    preview: ActResultPreview | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        journal_seq: int | None | Unset
        if isinstance(self.journal_seq, Unset):
            journal_seq = UNSET
        else:
            journal_seq = self.journal_seq

        event_id: None | str | Unset
        if isinstance(self.event_id, Unset):
            event_id = UNSET
        else:
            event_id = self.event_id

        dry_run = self.dry_run

        preview: dict[str, Any] | Unset = UNSET
        if not isinstance(self.preview, Unset):
            preview = self.preview.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if journal_seq is not UNSET:
            field_dict["journal_seq"] = journal_seq
        if event_id is not UNSET:
            field_dict["event_id"] = event_id
        if dry_run is not UNSET:
            field_dict["dry_run"] = dry_run
        if preview is not UNSET:
            field_dict["preview"] = preview

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.act_result_preview import ActResultPreview

        d = dict(src_dict)

        def _parse_journal_seq(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        journal_seq = _parse_journal_seq(d.pop("journal_seq", UNSET))

        def _parse_event_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        event_id = _parse_event_id(d.pop("event_id", UNSET))

        dry_run = d.pop("dry_run", UNSET)

        _preview = d.pop("preview", UNSET)
        preview: ActResultPreview | Unset
        if isinstance(_preview, Unset):
            preview = UNSET
        else:
            preview = ActResultPreview.from_dict(_preview)

        act_result = cls(
            journal_seq=journal_seq,
            event_id=event_id,
            dry_run=dry_run,
            preview=preview,
        )

        act_result.additional_properties = d
        return act_result

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
