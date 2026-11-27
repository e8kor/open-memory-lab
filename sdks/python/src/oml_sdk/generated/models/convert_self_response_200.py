from __future__ import annotations

from collections.abc import Mapping
from typing import (
    Any,
    Literal,
    Self,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define

T = TypeVar("T", bound="ConvertSelfResponse200")


@_attrs_define
class ConvertSelfResponse200:
    """
    Attributes:
        key_id (str):
        name (None | str):
        role (Literal['maintenance']):
        requests_per_minute (int | None):
    """

    key_id: str
    name: None | str
    role: Literal["maintenance"]
    requests_per_minute: int | None

    def to_dict(self) -> dict[str, Any]:
        key_id = self.key_id

        name: None | str
        name = self.name

        role = self.role

        requests_per_minute: int | None
        requests_per_minute = self.requests_per_minute

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "key_id": key_id,
                "name": name,
                "role": role,
                "requests_per_minute": requests_per_minute,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        key_id = d.pop("key_id")

        def _parse_name(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        name = _parse_name(d.pop("name"))

        role = cast(Literal["maintenance"], d.pop("role"))
        if role != "maintenance":
            raise ValueError(f"role must match const 'maintenance', got '{role}'")

        def _parse_requests_per_minute(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        requests_per_minute = _parse_requests_per_minute(d.pop("requests_per_minute"))

        convert_self_response_200 = cls(
            key_id=key_id,
            name=name,
            role=role,
            requests_per_minute=requests_per_minute,
        )

        return convert_self_response_200
