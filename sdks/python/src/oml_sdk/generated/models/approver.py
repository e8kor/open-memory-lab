from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.approver_act import ApproverAct
from ..models.approver_used_by import ApproverUsedBy
from ..types import UNSET, Unset

T = TypeVar("T", bound="Approver")


@_attrs_define
class Approver:
    """The reviewer behind a version's approving verdict, by kind (ADR-0077 d2). It carries no key id, key name, principal
    or crew: who reviewed is in the journal's review events, which no reader of a memory is served (ADR-0025 d15;
    ADR-0037 d5). A crew's reviewer reads used_by agent; a person reads used_by person; a person approving their own
    agents' writes reads same_user true.

        Attributes:
            used_by (ApproverUsedBy): The reviewing key's attestation, who holds it, when it gave the verdict
                (docs/architecture/roles-and-keys.md), as a served guardrail's verdicts carry it (ADR-0037 d5).
            same_user (bool | None): True when the reviewing key and the authoring key act for one person, as the
                promotion's event marks it (ADR-0055 d7); the reviewing key and principal still differ from the author's
                (ADR-0044 d8). Null on a version ratified before the mark.
            act (ApproverAct | Unset): verdict for a version a review promoted; admin_fix for one served because of an
                admin's fix (ADR-0089 d7). Since v0.8.4.
            confirmed (bool | None | Unset): For act admin_fix, whether a person confirmed the fix; null for a verdict
                (ADR-0089 d7). Since v0.8.4.
            reverted (bool | None | Unset): For act admin_fix, whether a person reverted the fix; the version keeps its mark
                and reads confirmed false; null for a verdict (ADR-0089 d7, d17). Since v0.8.4.
    """

    used_by: ApproverUsedBy
    same_user: bool | None
    act: ApproverAct | Unset = UNSET
    confirmed: bool | None | Unset = UNSET
    reverted: bool | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        used_by = self.used_by.value

        same_user: bool | None
        same_user = self.same_user

        act: str | Unset = UNSET
        if not isinstance(self.act, Unset):
            act = self.act.value

        confirmed: bool | None | Unset
        if isinstance(self.confirmed, Unset):
            confirmed = UNSET
        else:
            confirmed = self.confirmed

        reverted: bool | None | Unset
        if isinstance(self.reverted, Unset):
            reverted = UNSET
        else:
            reverted = self.reverted

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "used_by": used_by,
                "same_user": same_user,
            }
        )
        if act is not UNSET:
            field_dict["act"] = act
        if confirmed is not UNSET:
            field_dict["confirmed"] = confirmed
        if reverted is not UNSET:
            field_dict["reverted"] = reverted

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        used_by = ApproverUsedBy(d.pop("used_by"))

        def _parse_same_user(data: object) -> bool | None:
            if data is None:
                return data
            return cast(bool | None, data)

        same_user = _parse_same_user(d.pop("same_user"))

        _act = d.pop("act", UNSET)
        act: ApproverAct | Unset
        if isinstance(_act, Unset):
            act = UNSET
        else:
            act = ApproverAct(_act)

        def _parse_confirmed(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        confirmed = _parse_confirmed(d.pop("confirmed", UNSET))

        def _parse_reverted(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        reverted = _parse_reverted(d.pop("reverted", UNSET))

        approver = cls(
            used_by=used_by,
            same_user=same_user,
            act=act,
            confirmed=confirmed,
            reverted=reverted,
        )

        return approver
