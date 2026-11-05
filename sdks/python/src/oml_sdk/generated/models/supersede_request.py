from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.severity import Severity
from ..models.supersede_request_kind import SupersedeRequestKind
from ..models.supersede_request_on_similar import SupersedeRequestOnSimilar
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.guardrail_rule import GuardrailRule


T = TypeVar("T", bound="SupersedeRequest")


@_attrs_define
class SupersedeRequest:
    """Unknown fields are refused (request.validation; ADR-0025 d16). The designed successor fields — attachments,
    simulation_id, simulated_time, mapping and the term fields — are declared under x-oml-designed-fields in openapi-
    designed.yaml, and each sent here answers its v0.3.0 code rather than the unknown-field refusal (ADR-0039 d9).

        Attributes:
            body (str): Claim text, at most submission.max_body_size (shipped 1 MiB) UTF-8 bytes of the NFC-normalized body;
                larger is 413 request.payload_too_large (http-api.md Limits). With retirement true, why the claim is retired.
            workarea (str | Unset): A workarea, by its node id, its name or its qualified name. Resolved server-side before
                any check that reads state; an unknown name answers exactly what an unknown id answers. A user key resolves a
                bare name in its own user root's namespace, plus universe and its root's name; every other key in the shared
                namespace (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is universe,
                is neither, and is refused (request.validation).
            retirement (bool | Unset): true proposes retiring the claim, and body states why. Refused with
                request.validation when the target's kind is term (terms deprecate), or when a kind is sent that differs from
                the target's. Default: False.
            kind (SupersedeRequestKind | Unset): Omitted, the successor takes the target's kind — the one field inherited
                from the target. A memory, derived_fact or steering target admits any of those three; a vocabulary-kind target
                and a retirement keep the target's kind; anything else is request.validation (docs/architecture/audit-and-
                lineage.md).
            derives_from (list[str] | Unset): The superseded target is included implicitly.
            reason (str | Unset):
            severity (Severity | Unset): A guardrail's severity: never and must bind, should is the reviewed default, and it
                orders the set first; required on kind guardrail except a retirement, refused on every other kind
                (request.validation, details.field /severity; ADR-0037 d2, d10; since v0.5.1).
            rule (GuardrailRule | Unset): A guardrail's optional rule in the oml-gr/1 grammar: one tool class, its match and
                an effect, deny or ask, never allow; should takes ask only. Refused on every other kind and on a retirement
                (request.validation, details.field /rule); a rule outside the grammar, or over 160 bytes, is vetoed as
                submission.scan_veto with guardrail.rule_unparsable or guardrail.too_long (ADR-0037 d8, d10; since v0.5.1).
            on_similar (SupersedeRequestOnSimilar | Unset): Omitted, hold. What the write does when the similar check names
                a row the key may read with the same text or a near copy (ADR-0050 d1, d17): hold captures it held and lists the
                rows in similar; refuse answers 409 submission.similar_exists with details.similar, journals nothing and claims
                no idempotency record. A retirement is never checked, and the claim superseded is never named. Not part of the
                request digest. Since v0.5.9.
    """

    body: str
    workarea: str | Unset = UNSET
    retirement: bool | Unset = False
    kind: SupersedeRequestKind | Unset = UNSET
    derives_from: list[str] | Unset = UNSET
    reason: str | Unset = UNSET
    severity: Severity | Unset = UNSET
    rule: GuardrailRule | Unset = UNSET
    on_similar: SupersedeRequestOnSimilar | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        body = self.body

        workarea: str | Unset
        if isinstance(self.workarea, Unset):
            workarea = UNSET
        else:
            workarea = self.workarea

        retirement = self.retirement

        kind: str | Unset = UNSET
        if not isinstance(self.kind, Unset):
            kind = self.kind.value

        derives_from: list[str] | Unset = UNSET
        if not isinstance(self.derives_from, Unset):
            derives_from = self.derives_from

        reason = self.reason

        severity: str | Unset = UNSET
        if not isinstance(self.severity, Unset):
            severity = self.severity.value

        rule: dict[str, Any] | Unset = UNSET
        if not isinstance(self.rule, Unset):
            rule = self.rule.to_dict()

        on_similar: str | Unset = UNSET
        if not isinstance(self.on_similar, Unset):
            on_similar = self.on_similar.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "body": body,
            }
        )
        if workarea is not UNSET:
            field_dict["workarea"] = workarea
        if retirement is not UNSET:
            field_dict["retirement"] = retirement
        if kind is not UNSET:
            field_dict["kind"] = kind
        if derives_from is not UNSET:
            field_dict["derives_from"] = derives_from
        if reason is not UNSET:
            field_dict["reason"] = reason
        if severity is not UNSET:
            field_dict["severity"] = severity
        if rule is not UNSET:
            field_dict["rule"] = rule
        if on_similar is not UNSET:
            field_dict["on_similar"] = on_similar

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.guardrail_rule import GuardrailRule

        d = dict(src_dict)
        body = d.pop("body")

        def _parse_workarea(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        workarea = _parse_workarea(d.pop("workarea", UNSET))

        retirement = d.pop("retirement", UNSET)

        _kind = d.pop("kind", UNSET)
        kind: SupersedeRequestKind | Unset
        if isinstance(_kind, Unset):
            kind = UNSET
        else:
            kind = SupersedeRequestKind(_kind)

        derives_from = cast(list[str], d.pop("derives_from", UNSET))

        reason = d.pop("reason", UNSET)

        _severity = d.pop("severity", UNSET)
        severity: Severity | Unset
        if isinstance(_severity, Unset):
            severity = UNSET
        else:
            severity = Severity(_severity)

        _rule = d.pop("rule", UNSET)
        rule: GuardrailRule | Unset
        if isinstance(_rule, Unset):
            rule = UNSET
        else:
            rule = GuardrailRule.from_dict(_rule)

        _on_similar = d.pop("on_similar", UNSET)
        on_similar: SupersedeRequestOnSimilar | Unset
        if isinstance(_on_similar, Unset):
            on_similar = UNSET
        else:
            on_similar = SupersedeRequestOnSimilar(_on_similar)

        supersede_request = cls(
            body=body,
            workarea=workarea,
            retirement=retirement,
            kind=kind,
            derives_from=derives_from,
            reason=reason,
            severity=severity,
            rule=rule,
            on_similar=on_similar,
        )

        return supersede_request
