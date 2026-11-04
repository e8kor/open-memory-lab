from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="SetSearchDefaultBody")


@_attrs_define
class SetSearchDefaultBody:
    """
    Attributes:
        pipeline (None | str | Unset): A served pipeline, or null to follow the deployment's default.
    """

    pipeline: None | str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        pipeline: None | str | Unset
        if isinstance(self.pipeline, Unset):
            pipeline = UNSET
        else:
            pipeline = self.pipeline

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if pipeline is not UNSET:
            field_dict["pipeline"] = pipeline

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_pipeline(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        pipeline = _parse_pipeline(d.pop("pipeline", UNSET))

        set_search_default_body = cls(
            pipeline=pipeline,
        )

        return set_search_default_body
