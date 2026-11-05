from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.source_check_request_result import SourceCheckRequestResult
from ..types import UNSET, Unset

T = TypeVar("T", bound="SourceCheckRequest")


@_attrs_define
class SourceCheckRequest:
    """
    Attributes:
        rev (str): The anchor's rev grammar: with a repo anchor, 40 or 64 lower-case hex digits.
        result (SourceCheckRequestResult):
        hash_ (str | Unset): The span's hash at rev under the anchor's hasher; refused on an anchor without one.
        path (str | Unset): For moved alone, required: the span's new path, under the anchor path grammar.
        fragment (str | Unset): For moved alone: the span's new fragment, under the fragment grammar.
    """

    rev: str
    result: SourceCheckRequestResult
    hash_: str | Unset = UNSET
    path: str | Unset = UNSET
    fragment: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        rev = self.rev

        result = self.result.value

        hash_ = self.hash_

        path = self.path

        fragment = self.fragment

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "rev": rev,
                "result": result,
            }
        )
        if hash_ is not UNSET:
            field_dict["hash"] = hash_
        if path is not UNSET:
            field_dict["path"] = path
        if fragment is not UNSET:
            field_dict["fragment"] = fragment

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        rev = d.pop("rev")

        result = SourceCheckRequestResult(d.pop("result"))

        hash_ = d.pop("hash", UNSET)

        path = d.pop("path", UNSET)

        fragment = d.pop("fragment", UNSET)

        source_check_request = cls(
            rev=rev,
            result=result,
            hash_=hash_,
            path=path,
            fragment=fragment,
        )

        return source_check_request
