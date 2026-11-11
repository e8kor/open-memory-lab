from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="RecallSearchWindowType0")


@_attrs_define
class RecallSearchWindowType0:
    """The date window read from the query by rule oml-when/1, anchored to the recall's now; null when the query names no
    date (ADR-0059 d6).

        Attributes:
            from_ (datetime.datetime | None): The window's first instant; null when open.
            until (datetime.datetime | None): The instant after its last; null when open.
            phrase (str): The query's words the window was read from.
    """

    from_: datetime.datetime | None
    until: datetime.datetime | None
    phrase: str

    def to_dict(self) -> dict[str, Any]:
        from_: None | str
        if isinstance(self.from_, datetime.datetime):
            from_ = self.from_.isoformat()
        else:
            from_ = self.from_

        until: None | str
        if isinstance(self.until, datetime.datetime):
            until = self.until.isoformat()
        else:
            until = self.until

        phrase = self.phrase

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "from": from_,
                "until": until,
                "phrase": phrase,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_from_(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                from_type_0 = datetime.datetime.fromisoformat(data)

                return from_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        from_ = _parse_from_(d.pop("from"))

        def _parse_until(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                until_type_0 = datetime.datetime.fromisoformat(data)

                return until_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        until = _parse_until(d.pop("until"))

        phrase = d.pop("phrase")

        recall_search_window_type_0 = cls(
            from_=from_,
            until=until,
            phrase=phrase,
        )

        return recall_search_window_type_0
