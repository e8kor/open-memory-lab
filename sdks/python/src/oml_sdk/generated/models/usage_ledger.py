from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.usage_ledger_rows_item import UsageLedgerRowsItem


T = TypeVar("T", bound="UsageLedger")


@_attrs_define
class UsageLedger:
    """
    Attributes:
        period (str):
        rows (list[UsageLedgerRowsItem]):
    """

    period: str
    rows: list[UsageLedgerRowsItem]

    def to_dict(self) -> dict[str, Any]:
        period = self.period

        rows = []
        for rows_item_data in self.rows:
            rows_item = rows_item_data.to_dict()
            rows.append(rows_item)

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "period": period,
                "rows": rows,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.usage_ledger_rows_item import UsageLedgerRowsItem

        d = dict(src_dict)
        period = d.pop("period")

        rows = []
        _rows = d.pop("rows")
        for rows_item_data in _rows:
            rows_item = UsageLedgerRowsItem.from_dict(rows_item_data)

            rows.append(rows_item)

        usage_ledger = cls(
            period=period,
            rows=rows,
        )

        return usage_ledger
