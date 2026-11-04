from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.supersede_request_kind import SupersedeRequestKind
from ..types import UNSET, Unset

T = TypeVar("T", bound="SupersedeRequest")


@_attrs_define
class SupersedeRequest:
    """Unknown fields are refused (request.validation; ADR-0025 d16). The designed successor fields — attachments,
    simulation_id, simulated_time, mapping and the term fields — are declared under x-oml-designed-fields in openapi-
    designed.yaml, and each sent here answers its v0.3.0 code rather than the unknown-field refusal (ADR-0039 d9).

        Attributes:
            body (str): Claim text, at most submission.max_body_size (shipped 1 MiB) UTF-8 bytes of the NFC-normalized body;
                larger is 413 request.payload_too_large (http-api.md Limits). With retirement true, why the claim is retired.
            workarea (str | Unset): A workarea, by its node id or its name. Resolved server-side before any check that reads
                state; an unknown name answers exactly what an unknown id answers. A derived path is neither, and is refused
                (request.validation).
            retirement (bool | Unset): true proposes retiring the claim, and body states why. Refused with
                request.validation when the target's kind is term (terms deprecate), or when a kind is sent that differs from
                the target's. Default: False.
            kind (SupersedeRequestKind | Unset): Omitted, the successor takes the target's kind — the one field inherited
                from the target. A memory, derived_fact or steering target admits any of those three; a vocabulary-kind target
                and a retirement keep the target's kind; anything else is request.validation (docs/architecture/audit-and-
                lineage.md).
            derives_from (list[str] | Unset): The superseded target is included implicitly.
            reason (str | Unset):
    """

    body: str
    workarea: str | Unset = UNSET
    retirement: bool | Unset = False
    kind: SupersedeRequestKind | Unset = UNSET
    derives_from: list[str] | Unset = UNSET
    reason: str | Unset = UNSET

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

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
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

        supersede_request = cls(
            body=body,
            workarea=workarea,
            retirement=retirement,
            kind=kind,
            derives_from=derives_from,
            reason=reason,
        )

        return supersede_request
