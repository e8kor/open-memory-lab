from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.catalog_count_type_1 import CatalogCountType1


T = TypeVar("T", bound="MemoryPageCounts")


@_attrs_define
class MemoryPageCounts:
    """The catalog's first page only: per kind, the entries every filter but kinds admits, over exactly the rows the page's
    predicate admits (ADR-0092 d6).

    """

    additional_properties: dict[str, CatalogCountType1 | int] = _attrs_field(
        init=False, factory=dict
    )

    def to_dict(self) -> dict[str, Any]:
        from ..models.catalog_count_type_1 import CatalogCountType1

        field_dict: dict[str, Any] = {}
        for prop_name, prop in self.additional_properties.items():
            if isinstance(prop, CatalogCountType1):
                field_dict[prop_name] = prop.to_dict()
            else:
                field_dict[prop_name] = prop

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.catalog_count_type_1 import CatalogCountType1

        d = dict(src_dict)
        memory_page_counts = cls()

        additional_properties = {}
        for prop_name, prop_dict in d.items():

            def _parse_additional_property(data: object) -> CatalogCountType1 | int:
                try:
                    if not isinstance(data, dict):
                        raise TypeError()
                    componentsschemas_catalog_count_type_1 = (
                        CatalogCountType1.from_dict(data)
                    )

                    return componentsschemas_catalog_count_type_1
                except (TypeError, ValueError, AttributeError, KeyError):
                    pass
                return cast(CatalogCountType1 | int, data)

            additional_property = _parse_additional_property(prop_dict)

            additional_properties[prop_name] = additional_property

        memory_page_counts.additional_properties = additional_properties
        return memory_page_counts

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> CatalogCountType1 | int:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: CatalogCountType1 | int) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
