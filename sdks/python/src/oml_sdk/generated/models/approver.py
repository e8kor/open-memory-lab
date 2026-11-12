from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.approver_held_by import ApproverHeldBy

T = TypeVar("T", bound="Approver")


@_attrs_define
class Approver:
    """The reviewer behind a version's approving verdict, by kind (ADR-0077 d2). It carries no key id, key name, principal
    or crew: who reviewed is in the journal's review events, which no reader of a memory is served (ADR-0025 d15;
    ADR-0037 d5). A crew's reviewer reads held_by agent; a person reads held_by human; a person approving their own
    agents' writes reads same_user true.

        Attributes:
            held_by (ApproverHeldBy): The reviewing key's attestation, who holds it, when it gave the verdict
                (docs/architecture/roles-and-keys.md), as a served guardrail's verdicts carry it (ADR-0037 d5).
            same_user (bool | None): True when the reviewing key and the authoring key act for one person, as the
                promotion's event marks it (ADR-0055 d7); the reviewing key and principal still differ from the author's
                (ADR-0044 d8). Null on a version ratified before the mark.
    """

    held_by: ApproverHeldBy
    same_user: bool | None

    def to_dict(self) -> dict[str, Any]:
        held_by = self.held_by.value

        same_user: bool | None
        same_user = self.same_user

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "held_by": held_by,
                "same_user": same_user,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        held_by = ApproverHeldBy(d.pop("held_by"))

        def _parse_same_user(data: object) -> bool | None:
            if data is None:
                return data
            return cast(bool | None, data)

        same_user = _parse_same_user(d.pop("same_user"))

        approver = cls(
            held_by=held_by,
            same_user=same_user,
        )

        return approver
