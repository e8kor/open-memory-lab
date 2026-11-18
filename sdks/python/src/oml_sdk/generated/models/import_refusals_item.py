from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.import_refusals_item_source import ImportRefusalsItemSource


T = TypeVar("T", bound="ImportRefusalsItem")


@_attrs_define
class ImportRefusalsItem:
    """
    Attributes:
        claim_key (str):
        source (ImportRefusalsItemSource):
        code (str):
    """

    claim_key: str
    source: ImportRefusalsItemSource
    code: str

    def to_dict(self) -> dict[str, Any]:
        claim_key = self.claim_key

        source = self.source.to_dict()

        code = self.code

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "claim_key": claim_key,
                "source": source,
                "code": code,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.import_refusals_item_source import ImportRefusalsItemSource

        d = dict(src_dict)
        claim_key = d.pop("claim_key")

        source = ImportRefusalsItemSource.from_dict(d.pop("source"))

        code = d.pop("code")

        import_refusals_item = cls(
            claim_key=claim_key,
            source=source,
            code=code,
        )

        return import_refusals_item
