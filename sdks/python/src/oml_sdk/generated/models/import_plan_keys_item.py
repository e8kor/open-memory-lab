from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.import_plan_keys_item_outcome import ImportPlanKeysItemOutcome

if TYPE_CHECKING:
    from ..models.import_plan_keys_item_source_type_0 import (
        ImportPlanKeysItemSourceType0,
    )


T = TypeVar("T", bound="ImportPlanKeysItem")


@_attrs_define
class ImportPlanKeysItem:
    """
    Attributes:
        claim_key (str):
        outcome (ImportPlanKeysItemOutcome):
        source (ImportPlanKeysItemSourceType0 | None):
        expected_version (int | None):
    """

    claim_key: str
    outcome: ImportPlanKeysItemOutcome
    source: ImportPlanKeysItemSourceType0 | None
    expected_version: int | None

    def to_dict(self) -> dict[str, Any]:
        from ..models.import_plan_keys_item_source_type_0 import (
            ImportPlanKeysItemSourceType0,
        )

        claim_key = self.claim_key

        outcome = self.outcome.value

        source: dict[str, Any] | None
        if isinstance(self.source, ImportPlanKeysItemSourceType0):
            source = self.source.to_dict()
        else:
            source = self.source

        expected_version: int | None
        expected_version = self.expected_version

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "claim_key": claim_key,
                "outcome": outcome,
                "source": source,
                "expected_version": expected_version,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.import_plan_keys_item_source_type_0 import (
            ImportPlanKeysItemSourceType0,
        )

        d = dict(src_dict)
        claim_key = d.pop("claim_key")

        outcome = ImportPlanKeysItemOutcome(d.pop("outcome"))

        def _parse_source(data: object) -> ImportPlanKeysItemSourceType0 | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                source_type_0 = ImportPlanKeysItemSourceType0.from_dict(data)

                return source_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(ImportPlanKeysItemSourceType0 | None, data)

        source = _parse_source(d.pop("source"))

        def _parse_expected_version(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        expected_version = _parse_expected_version(d.pop("expected_version"))

        import_plan_keys_item = cls(
            claim_key=claim_key,
            outcome=outcome,
            source=source,
            expected_version=expected_version,
        )

        return import_plan_keys_item
