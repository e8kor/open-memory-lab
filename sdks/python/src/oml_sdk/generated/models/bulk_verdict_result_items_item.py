from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.bulk_verdict_result_items_item_outcome import (
    BulkVerdictResultItemsItemOutcome,
)
from ..models.bulk_verdict_result_items_item_state import (
    BulkVerdictResultItemsItemState,
)
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.bulk_verdict_result_items_item_details_type_0 import (
        BulkVerdictResultItemsItemDetailsType0,
    )


T = TypeVar("T", bound="BulkVerdictResultItemsItem")


@_attrs_define
class BulkVerdictResultItemsItem:
    """
    Attributes:
        artifact_id (UUID):
        outcome (BulkVerdictResultItemsItemOutcome):
        state (BulkVerdictResultItemsItemState | Unset): A decided item's state.
        version (int | Unset):
        journal_seq (int | Unset): The seq of the item's last event.
        same_user (bool | Unset):
        replayed (bool | Unset): true when the item answers its idempotency record (ADR-0055 d12).
        code (str | Unset): A skipped item's refusal, as POST /v2/review/verdicts would answer it.
        message (str | Unset):
        details (BulkVerdictResultItemsItemDetailsType0 | None | Unset):
    """

    artifact_id: UUID
    outcome: BulkVerdictResultItemsItemOutcome
    state: BulkVerdictResultItemsItemState | Unset = UNSET
    version: int | Unset = UNSET
    journal_seq: int | Unset = UNSET
    same_user: bool | Unset = UNSET
    replayed: bool | Unset = UNSET
    code: str | Unset = UNSET
    message: str | Unset = UNSET
    details: BulkVerdictResultItemsItemDetailsType0 | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.bulk_verdict_result_items_item_details_type_0 import (
            BulkVerdictResultItemsItemDetailsType0,
        )

        artifact_id = str(self.artifact_id)

        outcome = self.outcome.value

        state: str | Unset = UNSET
        if not isinstance(self.state, Unset):
            state = self.state.value

        version = self.version

        journal_seq = self.journal_seq

        same_user = self.same_user

        replayed = self.replayed

        code = self.code

        message = self.message

        details: dict[str, Any] | None | Unset
        if isinstance(self.details, Unset):
            details = UNSET
        elif isinstance(self.details, BulkVerdictResultItemsItemDetailsType0):
            details = self.details.to_dict()
        else:
            details = self.details

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "artifact_id": artifact_id,
                "outcome": outcome,
            }
        )
        if state is not UNSET:
            field_dict["state"] = state
        if version is not UNSET:
            field_dict["version"] = version
        if journal_seq is not UNSET:
            field_dict["journal_seq"] = journal_seq
        if same_user is not UNSET:
            field_dict["same_user"] = same_user
        if replayed is not UNSET:
            field_dict["replayed"] = replayed
        if code is not UNSET:
            field_dict["code"] = code
        if message is not UNSET:
            field_dict["message"] = message
        if details is not UNSET:
            field_dict["details"] = details

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.bulk_verdict_result_items_item_details_type_0 import (
            BulkVerdictResultItemsItemDetailsType0,
        )

        d = dict(src_dict)
        artifact_id = UUID(d.pop("artifact_id"))

        outcome = BulkVerdictResultItemsItemOutcome(d.pop("outcome"))

        _state = d.pop("state", UNSET)
        state: BulkVerdictResultItemsItemState | Unset
        if isinstance(_state, Unset):
            state = UNSET
        else:
            state = BulkVerdictResultItemsItemState(_state)

        version = d.pop("version", UNSET)

        journal_seq = d.pop("journal_seq", UNSET)

        same_user = d.pop("same_user", UNSET)

        replayed = d.pop("replayed", UNSET)

        code = d.pop("code", UNSET)

        message = d.pop("message", UNSET)

        def _parse_details(
            data: object,
        ) -> BulkVerdictResultItemsItemDetailsType0 | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                details_type_0 = BulkVerdictResultItemsItemDetailsType0.from_dict(data)

                return details_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(BulkVerdictResultItemsItemDetailsType0 | None | Unset, data)

        details = _parse_details(d.pop("details", UNSET))

        bulk_verdict_result_items_item = cls(
            artifact_id=artifact_id,
            outcome=outcome,
            state=state,
            version=version,
            journal_seq=journal_seq,
            same_user=same_user,
            replayed=replayed,
            code=code,
            message=message,
            details=details,
        )

        bulk_verdict_result_items_item.additional_properties = d
        return bulk_verdict_result_items_item

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
