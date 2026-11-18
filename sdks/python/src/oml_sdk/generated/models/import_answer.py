from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.import_ import Import
    from ..models.import_plan import ImportPlan


T = TypeVar("T", bound="ImportAnswer")


@_attrs_define
class ImportAnswer:
    """A dry run's plan, or the finished import.

    Attributes:
        plan (ImportPlan | Unset): What an import would write (ADR-0084 d8, R10).
        import_ (Import | Unset): One import as its importing key reads it (ADR-0084 d11, d15).
    """

    plan: ImportPlan | Unset = UNSET
    import_: Import | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        plan: dict[str, Any] | Unset = UNSET
        if not isinstance(self.plan, Unset):
            plan = self.plan.to_dict()

        import_: dict[str, Any] | Unset = UNSET
        if not isinstance(self.import_, Unset):
            import_ = self.import_.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if plan is not UNSET:
            field_dict["plan"] = plan
        if import_ is not UNSET:
            field_dict["import"] = import_

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.import_ import Import
        from ..models.import_plan import ImportPlan

        d = dict(src_dict)
        _plan = d.pop("plan", UNSET)
        plan: ImportPlan | Unset
        if isinstance(_plan, Unset):
            plan = UNSET
        else:
            plan = ImportPlan.from_dict(_plan)

        _import_ = d.pop("import", UNSET)
        import_: Import | Unset
        if isinstance(_import_, Unset):
            import_ = UNSET
        else:
            import_ = Import.from_dict(_import_)

        import_answer = cls(
            plan=plan,
            import_=import_,
        )

        return import_answer
