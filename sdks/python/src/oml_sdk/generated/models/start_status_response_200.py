from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.start_status_response_200_first_memory_type_1 import (
    StartStatusResponse200FirstMemoryType1,
)
from ..models.start_status_response_200_first_memory_type_2_type_1 import (
    StartStatusResponse200FirstMemoryType2Type1,
)
from ..models.start_status_response_200_first_memory_type_3_type_1 import (
    StartStatusResponse200FirstMemoryType3Type1,
)

T = TypeVar("T", bound="StartStatusResponse200")


@_attrs_define
class StartStatusResponse200:
    """
    Attributes:
        connected (bool):
        first_memory (None | StartStatusResponse200FirstMemoryType1 | StartStatusResponse200FirstMemoryType2Type1 |
            StartStatusResponse200FirstMemoryType3Type1):
    """

    connected: bool
    first_memory: (
        None
        | StartStatusResponse200FirstMemoryType1
        | StartStatusResponse200FirstMemoryType2Type1
        | StartStatusResponse200FirstMemoryType3Type1
    )
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        connected = self.connected

        first_memory: None | str
        if (
            isinstance(self.first_memory, StartStatusResponse200FirstMemoryType1)
            or isinstance(
                self.first_memory, StartStatusResponse200FirstMemoryType2Type1
            )
            or isinstance(
                self.first_memory, StartStatusResponse200FirstMemoryType3Type1
            )
        ):
            first_memory = self.first_memory.value
        else:
            first_memory = self.first_memory

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "connected": connected,
                "first_memory": first_memory,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        connected = d.pop("connected")

        def _parse_first_memory(
            data: object,
        ) -> (
            None
            | StartStatusResponse200FirstMemoryType1
            | StartStatusResponse200FirstMemoryType2Type1
            | StartStatusResponse200FirstMemoryType3Type1
        ):
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                first_memory_type_1 = StartStatusResponse200FirstMemoryType1(data)

                return first_memory_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                first_memory_type_2_type_1 = (
                    StartStatusResponse200FirstMemoryType2Type1(data)
                )

                return first_memory_type_2_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                first_memory_type_3_type_1 = (
                    StartStatusResponse200FirstMemoryType3Type1(data)
                )

                return first_memory_type_3_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(
                None
                | StartStatusResponse200FirstMemoryType1
                | StartStatusResponse200FirstMemoryType2Type1
                | StartStatusResponse200FirstMemoryType3Type1,
                data,
            )

        first_memory = _parse_first_memory(d.pop("first_memory"))

        start_status_response_200 = cls(
            connected=connected,
            first_memory=first_memory,
        )

        start_status_response_200.additional_properties = d
        return start_status_response_200

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
