from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.whoami_person_start_type_0_first_memory_type_1 import (
    WhoamiPersonStartType0FirstMemoryType1,
)
from ..models.whoami_person_start_type_0_first_memory_type_2_type_1 import (
    WhoamiPersonStartType0FirstMemoryType2Type1,
)
from ..models.whoami_person_start_type_0_first_memory_type_3_type_1 import (
    WhoamiPersonStartType0FirstMemoryType3Type1,
)
from ..types import UNSET, Unset

T = TypeVar("T", bound="WhoamiPersonStartType0")


@_attrs_define
class WhoamiPersonStartType0:
    """With start=true, what the onboarding page waits for: whether one of the person's agent keys has connected, and the
    state of the newest memory of their root, held or promoted, else null (ADR-0068 d10; ADR-0070 d5). Absent otherwise.
    Since v0.7.0.

        Attributes:
            connected (bool | Unset):
            first_memory (None | Unset | WhoamiPersonStartType0FirstMemoryType1 |
                WhoamiPersonStartType0FirstMemoryType2Type1 | WhoamiPersonStartType0FirstMemoryType3Type1):
    """

    connected: bool | Unset = UNSET
    first_memory: (
        None
        | Unset
        | WhoamiPersonStartType0FirstMemoryType1
        | WhoamiPersonStartType0FirstMemoryType2Type1
        | WhoamiPersonStartType0FirstMemoryType3Type1
    ) = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        connected = self.connected

        first_memory: None | str | Unset
        if isinstance(self.first_memory, Unset):
            first_memory = UNSET
        elif (
            isinstance(self.first_memory, WhoamiPersonStartType0FirstMemoryType1)
            or isinstance(
                self.first_memory, WhoamiPersonStartType0FirstMemoryType2Type1
            )
            or isinstance(
                self.first_memory, WhoamiPersonStartType0FirstMemoryType3Type1
            )
        ):
            first_memory = self.first_memory.value
        else:
            first_memory = self.first_memory

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if connected is not UNSET:
            field_dict["connected"] = connected
        if first_memory is not UNSET:
            field_dict["first_memory"] = first_memory

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        connected = d.pop("connected", UNSET)

        def _parse_first_memory(
            data: object,
        ) -> (
            None
            | Unset
            | WhoamiPersonStartType0FirstMemoryType1
            | WhoamiPersonStartType0FirstMemoryType2Type1
            | WhoamiPersonStartType0FirstMemoryType3Type1
        ):
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                first_memory_type_1 = WhoamiPersonStartType0FirstMemoryType1(data)

                return first_memory_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                first_memory_type_2_type_1 = (
                    WhoamiPersonStartType0FirstMemoryType2Type1(data)
                )

                return first_memory_type_2_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                first_memory_type_3_type_1 = (
                    WhoamiPersonStartType0FirstMemoryType3Type1(data)
                )

                return first_memory_type_3_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(
                None
                | Unset
                | WhoamiPersonStartType0FirstMemoryType1
                | WhoamiPersonStartType0FirstMemoryType2Type1
                | WhoamiPersonStartType0FirstMemoryType3Type1,
                data,
            )

        first_memory = _parse_first_memory(d.pop("first_memory", UNSET))

        whoami_person_start_type_0 = cls(
            connected=connected,
            first_memory=first_memory,
        )

        whoami_person_start_type_0.additional_properties = d
        return whoami_person_start_type_0

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
