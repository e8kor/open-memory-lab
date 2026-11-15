from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.test_fields_shape import TestFieldsShape

T = TypeVar("T", bound="TestFields")


@_attrs_define
class TestFields:
    """A test case's shape (ADR-0086 d1).

    Attributes:
        shape (TestFieldsShape):
    """

    shape: TestFieldsShape

    def to_dict(self) -> dict[str, Any]:
        shape = self.shape.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "shape": shape,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        shape = TestFieldsShape(d.pop("shape"))

        test_fields = cls(
            shape=shape,
        )

        return test_fields
