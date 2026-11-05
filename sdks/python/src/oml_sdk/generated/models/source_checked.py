from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.source_checked_result import SourceCheckedResult

if TYPE_CHECKING:
    from ..models.source_checked_moved_to_type_0 import SourceCheckedMovedToType0


T = TypeVar("T", bound="SourceChecked")


@_attrs_define
class SourceChecked:
    """
    Attributes:
        rev (str): The commit or page version the span was compared at.
        at (datetime.datetime):
        result (SourceCheckedResult):
        moved_to (None | SourceCheckedMovedToType0): Where a moved check found the span (ADR-0057 d18); null for any
            other result.
    """

    rev: str
    at: datetime.datetime
    result: SourceCheckedResult
    moved_to: None | SourceCheckedMovedToType0

    def to_dict(self) -> dict[str, Any]:
        from ..models.source_checked_moved_to_type_0 import SourceCheckedMovedToType0

        rev = self.rev

        at = self.at.isoformat()

        result = self.result.value

        moved_to: dict[str, Any] | None
        if isinstance(self.moved_to, SourceCheckedMovedToType0):
            moved_to = self.moved_to.to_dict()
        else:
            moved_to = self.moved_to

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "rev": rev,
                "at": at,
                "result": result,
                "moved_to": moved_to,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.source_checked_moved_to_type_0 import SourceCheckedMovedToType0

        d = dict(src_dict)
        rev = d.pop("rev")

        at = datetime.datetime.fromisoformat(d.pop("at"))

        result = SourceCheckedResult(d.pop("result"))

        def _parse_moved_to(data: object) -> None | SourceCheckedMovedToType0:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                moved_to_type_0 = SourceCheckedMovedToType0.from_dict(data)

                return moved_to_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | SourceCheckedMovedToType0, data)

        moved_to = _parse_moved_to(d.pop("moved_to"))

        source_checked = cls(
            rev=rev,
            at=at,
            result=result,
            moved_to=moved_to,
        )

        return source_checked
