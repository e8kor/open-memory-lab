from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.severity import Severity
from ..models.supersede_request_kind import SupersedeRequestKind
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.anchor import Anchor
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
            expected_version (int): The claim's latest version the caller can read, as it last read it: its promoted
                versions where it reads the workarea and its own keys' versions; never another author's held version. Any other
                is 409 submission.version_conflict, details.current_version (ADR-0057 d19).
            anchor (Anchor | Unset): The origin the author declares (ADR-0057 d1-d3, d21): signed in the submission event,
                immutable per version, shown to the reviewer, never fetched by the server, and clear metadata that crypto-
                shredding never erases. Exactly one of repo (with rev and path) and uri; hash and hasher together or neither.
                Any other shape is request.validation naming the field, such as /anchor/path; a scan finding in repo, uri, path
                or fragment is submission.scan_veto.
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
            valid_from (str | Unset): YYYY, YYYY-MM, YYYY-MM-DD, or an RFC 3339 date-time with an offset and at most six
                fractional digits. A period is read in UTC: 2026-11-05 runs from 00:00 to 24:00 UTC (proposed ADR-0059 d2).
            valid_until (str | Unset): YYYY, YYYY-MM, YYYY-MM-DD, or an RFC 3339 date-time with an offset and at most six
                fractional digits. A period is read in UTC: 2026-11-05 runs from 00:00 to 24:00 UTC (proposed ADR-0059 d2).
            format_ (str | Unset): The media type of body, signed with the successor and fixed for it; nothing is inherited
                from the target, so a format changes by supersession alone: text/plain (the default), text/markdown,
                text/vnd.oml.mermaid, application/json, application/yaml, or text/vnd.oml.code; lang=<language>. An unknown
                value is request.validation with details.field format and details.allowed; a source its format refuses is 422
                submission.format_invalid; kinds other than memory and derived_fact take text/plain or text/markdown only
                (ADR-0056 d1, d13).
            title (str | Unset): One line stating the one fact the body shows, NFC, at most 100 characters, signed with the
                version. Required with a format other than text/plain and text/markdown: request.validation with details.field
                title otherwise (ADR-0056 d10).
    """

    body: str
    expected_version: int
    anchor: Anchor | Unset = UNSET
    workarea: str | Unset = UNSET
    retirement: bool | Unset = False
    kind: SupersedeRequestKind | Unset = UNSET
    derives_from: list[str] | Unset = UNSET
    reason: str | Unset = UNSET
    severity: Severity | Unset = UNSET
    rule: GuardrailRule | Unset = UNSET
    valid_from: str | Unset = UNSET
    valid_until: str | Unset = UNSET
    format_: str | Unset = UNSET
    title: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        body = self.body

        expected_version = self.expected_version

        anchor: dict[str, Any] | Unset = UNSET
        if not isinstance(self.anchor, Unset):
            anchor = self.anchor.to_dict()

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

        valid_from = self.valid_from

        valid_until = self.valid_until

        format_ = self.format_

        title = self.title

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "body": body,
                "expected_version": expected_version,
            }
        )
        if anchor is not UNSET:
            field_dict["anchor"] = anchor
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
        if valid_from is not UNSET:
            field_dict["valid_from"] = valid_from
        if valid_until is not UNSET:
            field_dict["valid_until"] = valid_until
        if format_ is not UNSET:
            field_dict["format"] = format_
        if title is not UNSET:
            field_dict["title"] = title

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.anchor import Anchor
        from ..models.guardrail_rule import GuardrailRule

        d = dict(src_dict)
        body = d.pop("body")

        expected_version = d.pop("expected_version")

        _anchor = d.pop("anchor", UNSET)
        anchor: Anchor | Unset
        if isinstance(_anchor, Unset):
            anchor = UNSET
        else:
            anchor = Anchor.from_dict(_anchor)

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

        valid_from = d.pop("valid_from", UNSET)

        valid_until = d.pop("valid_until", UNSET)

        format_ = d.pop("format", UNSET)

        title = d.pop("title", UNSET)

        supersede_request = cls(
            body=body,
            expected_version=expected_version,
            anchor=anchor,
            workarea=workarea,
            retirement=retirement,
            kind=kind,
            derives_from=derives_from,
            reason=reason,
            severity=severity,
            rule=rule,
            valid_from=valid_from,
            valid_until=valid_until,
            format_=format_,
            title=title,
        )

        return supersede_request
