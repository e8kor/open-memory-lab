from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.severity import Severity
from ..models.submit_request_kind import SubmitRequestKind
from ..models.submit_request_on_similar import SubmitRequestOnSimilar
from ..models.submit_request_verb import SubmitRequestVerb
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.anchor import Anchor
    from ..models.guardrail_rule import GuardrailRule
    from ..models.origin import Origin


T = TypeVar("T", bound="SubmitRequest")


@_attrs_define
class SubmitRequest:
    """The target is workarea, else the key's default_workarea; neither present is refused with request.workarea_unresolved
    (ADR-0039 d4, d7). This body, and its nested objects, stay open: ADR-0025 d16 closes the recall, supersede, key-
    issuance and grant bodies only. The designed attachment, simulation and vocabulary fields are declared under x-oml-
    designed-fields in openapi-designed.yaml; each sent here answers its v0.3.0 code (ADR-0039 d9).

        Attributes:
            body (str): Text body — claim text, at most submission.max_body_size (shipped 1 MiB) UTF-8 bytes of the NFC-
                normalized body; larger is 413 request.payload_too_large (http-api.md Limits). Binary content goes through the
                designed attachments (openapi-designed.yaml).
            verb (SubmitRequestVerb | Unset): Omitted, learn when derives_from is non-empty and remember otherwise (ADR-0039
                d3); a sent verb is honoured. learn requires non-empty derives_from — refused with submission.lineage_required
                otherwise; a derived conclusion without nameable lineage is remember with kind derived_fact.
            claim_key (str | Unset): Groups versions of one claim; one live claim per (tenant, workarea): a new claim
                collides with a held, promoted or demoted version (submission.claim_key_conflict), and a claim whose every
                version is rejected or erased releases the key (docs/architecture/audit-and-lineage.md). Omitted, the server
                derives auto- plus the first 16 lowercase hex digits of SHA-256(the key's id, a zero byte, the NFC body), so a
                conflict only ever names the writer's own rows; a sent claim_key starting auto- is refused with
                request.validation, details.field /claim_key (ADR-0039 d6).
            kind (SubmitRequestKind | Unset): guardrail requires severity and admits rule (ADR-0037 d10; since v0.5.1).
                Default: SubmitRequestKind.MEMORY.
            valid_from (str | Unset): YYYY, YYYY-MM, YYYY-MM-DD, or an RFC 3339 date-time with an offset and at most six
                fractional digits. A period is read in UTC: 2026-11-05 runs from 00:00 to 24:00 UTC (proposed ADR-0059 d2).
            valid_until (str | Unset): YYYY, YYYY-MM, YYYY-MM-DD, or an RFC 3339 date-time with an offset and at most six
                fractional digits. A period is read in UTC: 2026-11-05 runs from 00:00 to 24:00 UTC (proposed ADR-0059 d2).
            severity (Severity | Unset): A guardrail's severity: never and must bind, should is the reviewed default, and it
                orders the set first; required on kind guardrail except a retirement, refused on every other kind
                (request.validation, details.field /severity; ADR-0037 d2, d10; since v0.5.1).
            rule (GuardrailRule | Unset): A guardrail's optional rule in the oml-gr/1 grammar: one tool class, its match and
                an effect, deny or ask, never allow; should takes ask only. Refused on every other kind and on a retirement
                (request.validation, details.field /rule); a rule outside the grammar, or over 160 bytes, is vetoed as
                submission.scan_veto with guardrail.rule_unparsable or guardrail.too_long (ADR-0037 d8, d10; since v0.5.1).
            workarea (str | Unset): A workarea, by its node id, its name or its qualified name. Resolved server-side before
                any check that reads state; an unknown name answers exactly what an unknown id answers. A user key resolves a
                bare name in its own user root's namespace, plus universe and its root's name; every other key in the shared
                namespace (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is universe,
                is neither, and is refused (request.validation).
            derives_from (list[str] | Unset): Artifact ids; the declared lineage (omission is undetectable — honesty rule).
            on_similar (SubmitRequestOnSimilar | Unset): Omitted, hold. What the write does when the similar check names a
                row the key may read with the same text or a near copy (ADR-0050 d1, d17): hold captures it held and lists the
                rows in similar; refuse answers 409 submission.similar_exists with details.similar, journals nothing and claims
                no idempotency record. Not part of the request digest. Since v0.5.9.
            anchor (Anchor | Unset): The origin the author declares (ADR-0057 d1-d3, d21): signed in the submission event,
                immutable per version, shown to the reviewer, never fetched by the server, and clear metadata that crypto-
                shredding never erases. Exactly one of repo (with rev and path) and uri; hash and hasher together or neither.
                Any other shape is request.validation naming the field, such as /anchor/path; a scan finding in repo, uri, path
                or fragment is submission.scan_veto.
            origin (Origin | Unset): What kind of source the text was taken from (ADR-0063 d1, d2): signed in the submission
                event, immutable per version, never inherited by a successor. Omitted on a write, it is recorded manual. chat
                requires a chat anchor and a chat anchor requires chat; either mismatch is request.validation naming
                /origin/kind.
    """

    body: str
    verb: SubmitRequestVerb | Unset = UNSET
    claim_key: str | Unset = UNSET
    kind: SubmitRequestKind | Unset = SubmitRequestKind.MEMORY
    valid_from: str | Unset = UNSET
    valid_until: str | Unset = UNSET
    severity: Severity | Unset = UNSET
    rule: GuardrailRule | Unset = UNSET
    workarea: str | Unset = UNSET
    derives_from: list[str] | Unset = UNSET
    on_similar: SubmitRequestOnSimilar | Unset = UNSET
    anchor: Anchor | Unset = UNSET
    origin: Origin | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        body = self.body

        verb: str | Unset = UNSET
        if not isinstance(self.verb, Unset):
            verb = self.verb.value

        claim_key = self.claim_key

        kind: str | Unset = UNSET
        if not isinstance(self.kind, Unset):
            kind = self.kind.value

        valid_from = self.valid_from

        valid_until = self.valid_until

        severity: str | Unset = UNSET
        if not isinstance(self.severity, Unset):
            severity = self.severity.value

        rule: dict[str, Any] | Unset = UNSET
        if not isinstance(self.rule, Unset):
            rule = self.rule.to_dict()

        workarea: str | Unset
        if isinstance(self.workarea, Unset):
            workarea = UNSET
        else:
            workarea = self.workarea

        derives_from: list[str] | Unset = UNSET
        if not isinstance(self.derives_from, Unset):
            derives_from = self.derives_from

        on_similar: str | Unset = UNSET
        if not isinstance(self.on_similar, Unset):
            on_similar = self.on_similar.value

        anchor: dict[str, Any] | Unset = UNSET
        if not isinstance(self.anchor, Unset):
            anchor = self.anchor.to_dict()

        origin: dict[str, Any] | Unset = UNSET
        if not isinstance(self.origin, Unset):
            origin = self.origin.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "body": body,
            }
        )
        if verb is not UNSET:
            field_dict["verb"] = verb
        if claim_key is not UNSET:
            field_dict["claim_key"] = claim_key
        if kind is not UNSET:
            field_dict["kind"] = kind
        if valid_from is not UNSET:
            field_dict["valid_from"] = valid_from
        if valid_until is not UNSET:
            field_dict["valid_until"] = valid_until
        if severity is not UNSET:
            field_dict["severity"] = severity
        if rule is not UNSET:
            field_dict["rule"] = rule
        if workarea is not UNSET:
            field_dict["workarea"] = workarea
        if derives_from is not UNSET:
            field_dict["derives_from"] = derives_from
        if on_similar is not UNSET:
            field_dict["on_similar"] = on_similar
        if anchor is not UNSET:
            field_dict["anchor"] = anchor
        if origin is not UNSET:
            field_dict["origin"] = origin

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.anchor import Anchor
        from ..models.guardrail_rule import GuardrailRule
        from ..models.origin import Origin

        d = dict(src_dict)
        body = d.pop("body")

        _verb = d.pop("verb", UNSET)
        verb: SubmitRequestVerb | Unset
        if isinstance(_verb, Unset):
            verb = UNSET
        else:
            verb = SubmitRequestVerb(_verb)

        claim_key = d.pop("claim_key", UNSET)

        _kind = d.pop("kind", UNSET)
        kind: SubmitRequestKind | Unset
        if isinstance(_kind, Unset):
            kind = UNSET
        else:
            kind = SubmitRequestKind(_kind)

        valid_from = d.pop("valid_from", UNSET)

        valid_until = d.pop("valid_until", UNSET)

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

        def _parse_workarea(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        workarea = _parse_workarea(d.pop("workarea", UNSET))

        derives_from = cast(list[str], d.pop("derives_from", UNSET))

        _on_similar = d.pop("on_similar", UNSET)
        on_similar: SubmitRequestOnSimilar | Unset
        if isinstance(_on_similar, Unset):
            on_similar = UNSET
        else:
            on_similar = SubmitRequestOnSimilar(_on_similar)

        _anchor = d.pop("anchor", UNSET)
        anchor: Anchor | Unset
        if isinstance(_anchor, Unset):
            anchor = UNSET
        else:
            anchor = Anchor.from_dict(_anchor)

        _origin = d.pop("origin", UNSET)
        origin: Origin | Unset
        if isinstance(_origin, Unset):
            origin = UNSET
        else:
            origin = Origin.from_dict(_origin)

        submit_request = cls(
            body=body,
            verb=verb,
            claim_key=claim_key,
            kind=kind,
            valid_from=valid_from,
            valid_until=valid_until,
            severity=severity,
            rule=rule,
            workarea=workarea,
            derives_from=derives_from,
            on_similar=on_similar,
            anchor=anchor,
            origin=origin,
        )

        submit_request.additional_properties = d
        return submit_request

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
