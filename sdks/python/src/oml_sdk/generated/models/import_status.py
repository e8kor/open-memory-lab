from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.import_ import Import


T = TypeVar("T", bound="ImportStatus")


@_attrs_define
class ImportStatus:
    """
    Attributes:
        import_ (Import): One import as its importing key reads it (ADR-0084 d11, d15).
    """

    import_: Import

    def to_dict(self) -> dict[str, Any]:
        import_ = self.import_.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "import": import_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.import_ import Import

        d = dict(src_dict)
        import_ = Import.from_dict(d.pop("import"))

        import_status = cls(
            import_=import_,
        )

        return import_status
