from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="UsageEvalBody")


@_attrs_define
class UsageEvalBody:
    """
    Attributes:
        pipelines (list[str]):
    """

    pipelines: list[str]

    def to_dict(self) -> dict[str, Any]:
        pipelines = self.pipelines

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "pipelines": pipelines,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        pipelines = cast(list[str], d.pop("pipelines"))

        usage_eval_body = cls(
            pipelines=pipelines,
        )

        return usage_eval_body
