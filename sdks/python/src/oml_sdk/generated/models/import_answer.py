from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.import_plan import ImportPlan


T = TypeVar("T", bound="ImportAnswer")


@_attrs_define
class ImportAnswer:
    """A dry run's plan.

    Attributes:
        plan (ImportPlan): What an import would write (ADR-0084 d8, R10).
    """

    plan: ImportPlan

    def to_dict(self) -> dict[str, Any]:
        plan = self.plan.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "plan": plan,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.import_plan import ImportPlan

        d = dict(src_dict)
        plan = ImportPlan.from_dict(d.pop("plan"))

        import_answer = cls(
            plan=plan,
        )

        return import_answer
