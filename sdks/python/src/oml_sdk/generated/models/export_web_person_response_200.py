from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    Literal,
    Self,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.export_web_person_response_200_keys_item import (
        ExportWebPersonResponse200KeysItem,
    )
    from ..models.export_web_person_response_200_memories_item import (
        ExportWebPersonResponse200MemoriesItem,
    )


T = TypeVar("T", bound="ExportWebPersonResponse200")


@_attrs_define
class ExportWebPersonResponse200:
    """
    Attributes:
        format_ (Literal['oml-export/personal-1']):
        user (None | str):
        exported_at (datetime.datetime):
        memories (list[ExportWebPersonResponse200MemoriesItem]):
        keys (list[ExportWebPersonResponse200KeysItem]):
    """

    format_: Literal["oml-export/personal-1"]
    user: None | str
    exported_at: datetime.datetime
    memories: list[ExportWebPersonResponse200MemoriesItem]
    keys: list[ExportWebPersonResponse200KeysItem]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        format_ = self.format_

        user: None | str
        user = self.user

        exported_at = self.exported_at.isoformat()

        memories = []
        for memories_item_data in self.memories:
            memories_item = memories_item_data.to_dict()
            memories.append(memories_item)

        keys = []
        for keys_item_data in self.keys:
            keys_item = keys_item_data.to_dict()
            keys.append(keys_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "format": format_,
                "user": user,
                "exported_at": exported_at,
                "memories": memories,
                "keys": keys,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.export_web_person_response_200_keys_item import (
            ExportWebPersonResponse200KeysItem,
        )
        from ..models.export_web_person_response_200_memories_item import (
            ExportWebPersonResponse200MemoriesItem,
        )

        d = dict(src_dict)
        format_ = cast(Literal["oml-export/personal-1"], d.pop("format"))
        if format_ != "oml-export/personal-1":
            raise ValueError(
                f"format must match const 'oml-export/personal-1', got '{format_}'"
            )

        def _parse_user(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        user = _parse_user(d.pop("user"))

        exported_at = datetime.datetime.fromisoformat(d.pop("exported_at"))

        memories = []
        _memories = d.pop("memories")
        for memories_item_data in _memories:
            memories_item = ExportWebPersonResponse200MemoriesItem.from_dict(
                memories_item_data
            )

            memories.append(memories_item)

        keys = []
        _keys = d.pop("keys")
        for keys_item_data in _keys:
            keys_item = ExportWebPersonResponse200KeysItem.from_dict(keys_item_data)

            keys.append(keys_item)

        export_web_person_response_200 = cls(
            format_=format_,
            user=user,
            exported_at=exported_at,
            memories=memories,
            keys=keys,
        )

        export_web_person_response_200.additional_properties = d
        return export_web_person_response_200

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
