from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.served_guardrail_effect_inside_type_1 import (
    ServedGuardrailEffectInsideType1,
)
from ..models.served_guardrail_effect_inside_type_2_type_1 import (
    ServedGuardrailEffectInsideType2Type1,
)
from ..models.served_guardrail_effect_inside_type_3_type_1 import (
    ServedGuardrailEffectInsideType3Type1,
)
from ..models.severity import Severity
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.served_guardrail_admin_fix_type_0 import ServedGuardrailAdminFixType0
    from ..models.served_guardrail_verdicts_item import ServedGuardrailVerdictsItem


T = TypeVar("T", bound="ServedGuardrail")


@_attrs_define
class ServedGuardrail:
    """
    Attributes:
        artifact_id (str):
        claim_key (str):
        version (int):
        workspace_id (str):
        workspace_path (None | str):
        depth (int): The node's depth from the root, the root at 0.
        severity (Severity): A guardrail's severity: never and must bind, should is the reviewed default, and it orders
            the set first; required on kind guardrail except a retirement, refused on every other kind (request.validation,
            details.field /severity; ADR-0037 d2, d10; since v0.5.1).
        text (str):
        rule (Any): Its oml-gr/1 rule, or null.
        tokens (int):
        promoted_at (datetime.datetime | None): Null for a held version.
        unreviewed (bool): A held version, served to its author key or inside an in-progress workspace (ADR-0080 d10,
            d14).
        effect_inside (None | ServedGuardrailEffectInsideType1 | ServedGuardrailEffectInsideType2Type1 |
            ServedGuardrailEffectInsideType3Type1): The effect the hook applies in place of the rule's own: ask for every
            held version, null for a promoted one, which is never downgraded; the rule itself is unchanged (ADR-0080 d10).
        verdicts (list[ServedGuardrailVerdictsItem]): Each approving verdict's reviewer attestation and time, never the
            reviewer's key (ADR-0025 d15); empty for a held version.
        conflict (list[str]): The artifact_ids of the served rows of this claim_key with another severity; empty when
            none.
        admin_fix (None | ServedGuardrailAdminFixType0 | Unset): The mark of a guardrail an admin fix serves, whether a
            person confirmed or reverted the fix, never which admin; null for one a verdict promoted (ADR-0089 d7). Since
            v0.8.4.
    """

    artifact_id: str
    claim_key: str
    version: int
    workspace_id: str
    workspace_path: None | str
    depth: int
    severity: Severity
    text: str
    rule: Any
    tokens: int
    promoted_at: datetime.datetime | None
    unreviewed: bool
    effect_inside: (
        None
        | ServedGuardrailEffectInsideType1
        | ServedGuardrailEffectInsideType2Type1
        | ServedGuardrailEffectInsideType3Type1
    )
    verdicts: list[ServedGuardrailVerdictsItem]
    conflict: list[str]
    admin_fix: None | ServedGuardrailAdminFixType0 | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.served_guardrail_admin_fix_type_0 import (
            ServedGuardrailAdminFixType0,
        )

        artifact_id = self.artifact_id

        claim_key = self.claim_key

        version = self.version

        workspace_id = self.workspace_id

        workspace_path: None | str
        workspace_path = self.workspace_path

        depth = self.depth

        severity = self.severity.value

        text = self.text

        rule = self.rule

        tokens = self.tokens

        promoted_at: None | str
        if isinstance(self.promoted_at, datetime.datetime):
            promoted_at = self.promoted_at.isoformat()
        else:
            promoted_at = self.promoted_at

        unreviewed = self.unreviewed

        effect_inside: None | str
        if (
            isinstance(self.effect_inside, ServedGuardrailEffectInsideType1)
            or isinstance(self.effect_inside, ServedGuardrailEffectInsideType2Type1)
            or isinstance(self.effect_inside, ServedGuardrailEffectInsideType3Type1)
        ):
            effect_inside = self.effect_inside.value
        else:
            effect_inside = self.effect_inside

        verdicts = []
        for verdicts_item_data in self.verdicts:
            verdicts_item = verdicts_item_data.to_dict()
            verdicts.append(verdicts_item)

        conflict = self.conflict

        admin_fix: dict[str, Any] | None | Unset
        if isinstance(self.admin_fix, Unset):
            admin_fix = UNSET
        elif isinstance(self.admin_fix, ServedGuardrailAdminFixType0):
            admin_fix = self.admin_fix.to_dict()
        else:
            admin_fix = self.admin_fix

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "artifact_id": artifact_id,
                "claim_key": claim_key,
                "version": version,
                "workspace_id": workspace_id,
                "workspace_path": workspace_path,
                "depth": depth,
                "severity": severity,
                "text": text,
                "rule": rule,
                "tokens": tokens,
                "promoted_at": promoted_at,
                "unreviewed": unreviewed,
                "effect_inside": effect_inside,
                "verdicts": verdicts,
                "conflict": conflict,
            }
        )
        if admin_fix is not UNSET:
            field_dict["admin_fix"] = admin_fix

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.served_guardrail_admin_fix_type_0 import (
            ServedGuardrailAdminFixType0,
        )
        from ..models.served_guardrail_verdicts_item import ServedGuardrailVerdictsItem

        d = dict(src_dict)
        artifact_id = d.pop("artifact_id")

        claim_key = d.pop("claim_key")

        version = d.pop("version")

        workspace_id = d.pop("workspace_id")

        def _parse_workspace_path(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        workspace_path = _parse_workspace_path(d.pop("workspace_path"))

        depth = d.pop("depth")

        severity = Severity(d.pop("severity"))

        text = d.pop("text")

        rule = d.pop("rule")

        tokens = d.pop("tokens")

        def _parse_promoted_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                promoted_at_type_0 = datetime.datetime.fromisoformat(data)

                return promoted_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        promoted_at = _parse_promoted_at(d.pop("promoted_at"))

        unreviewed = d.pop("unreviewed")

        def _parse_effect_inside(
            data: object,
        ) -> (
            None
            | ServedGuardrailEffectInsideType1
            | ServedGuardrailEffectInsideType2Type1
            | ServedGuardrailEffectInsideType3Type1
        ):
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                effect_inside_type_1 = ServedGuardrailEffectInsideType1(data)

                return effect_inside_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                effect_inside_type_2_type_1 = ServedGuardrailEffectInsideType2Type1(
                    data
                )

                return effect_inside_type_2_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                effect_inside_type_3_type_1 = ServedGuardrailEffectInsideType3Type1(
                    data
                )

                return effect_inside_type_3_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(
                None
                | ServedGuardrailEffectInsideType1
                | ServedGuardrailEffectInsideType2Type1
                | ServedGuardrailEffectInsideType3Type1,
                data,
            )

        effect_inside = _parse_effect_inside(d.pop("effect_inside"))

        verdicts = []
        _verdicts = d.pop("verdicts")
        for verdicts_item_data in _verdicts:
            verdicts_item = ServedGuardrailVerdictsItem.from_dict(verdicts_item_data)

            verdicts.append(verdicts_item)

        conflict = cast(list[str], d.pop("conflict"))

        def _parse_admin_fix(
            data: object,
        ) -> None | ServedGuardrailAdminFixType0 | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                admin_fix_type_0 = ServedGuardrailAdminFixType0.from_dict(data)

                return admin_fix_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ServedGuardrailAdminFixType0 | Unset, data)

        admin_fix = _parse_admin_fix(d.pop("admin_fix", UNSET))

        served_guardrail = cls(
            artifact_id=artifact_id,
            claim_key=claim_key,
            version=version,
            workspace_id=workspace_id,
            workspace_path=workspace_path,
            depth=depth,
            severity=severity,
            text=text,
            rule=rule,
            tokens=tokens,
            promoted_at=promoted_at,
            unreviewed=unreviewed,
            effect_inside=effect_inside,
            verdicts=verdicts,
            conflict=conflict,
            admin_fix=admin_fix,
        )

        return served_guardrail
