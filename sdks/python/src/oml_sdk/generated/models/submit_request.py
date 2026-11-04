from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.submit_request_kind import SubmitRequestKind
from ..models.submit_request_verb import SubmitRequestVerb
from ..types import UNSET, Unset

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
            kind (SubmitRequestKind | Unset):  Default: SubmitRequestKind.MEMORY.
            workarea (str | Unset): A workarea, by its node id, its name or its qualified name. Resolved server-side before
                any check that reads state; an unknown name answers exactly what an unknown id answers. A user key resolves a
                bare name in its own user root's namespace, plus universe and its root's name; every other key in the shared
                namespace (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is universe,
                is neither, and is refused (request.validation).
            derives_from (list[str] | Unset): Artifact ids; the declared lineage (omission is undetectable — honesty rule).
    """

    body: str
    verb: SubmitRequestVerb | Unset = UNSET
    claim_key: str | Unset = UNSET
    kind: SubmitRequestKind | Unset = SubmitRequestKind.MEMORY
    workarea: str | Unset = UNSET
    derives_from: list[str] | Unset = UNSET
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

        workarea: str | Unset
        if isinstance(self.workarea, Unset):
            workarea = UNSET
        else:
            workarea = self.workarea

        derives_from: list[str] | Unset = UNSET
        if not isinstance(self.derives_from, Unset):
            derives_from = self.derives_from

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
        if workarea is not UNSET:
            field_dict["workarea"] = workarea
        if derives_from is not UNSET:
            field_dict["derives_from"] = derives_from

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
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

        def _parse_workarea(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        workarea = _parse_workarea(d.pop("workarea", UNSET))

        derives_from = cast(list[str], d.pop("derives_from", UNSET))

        submit_request = cls(
            body=body,
            verb=verb,
            claim_key=claim_key,
            kind=kind,
            workarea=workarea,
            derives_from=derives_from,
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
