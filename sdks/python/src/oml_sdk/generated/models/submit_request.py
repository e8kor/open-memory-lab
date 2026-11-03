from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.submit_request_kind import SubmitRequestKind
from ..models.submit_request_term_status import SubmitRequestTermStatus
from ..models.submit_request_verb import SubmitRequestVerb
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.submit_request_mapping import SubmitRequestMapping
    from ..models.submit_request_successors_item import SubmitRequestSuccessorsItem


T = TypeVar("T", bound="SubmitRequest")


@_attrs_define
class SubmitRequest:
    """Names no workarea: the target resolves from the key's default_workarea or the X-OML-Workarea header (ADR-0022 d4);
    neither present is refused with request.workarea_unresolved. This body, and its nested objects, stay open: ADR-0025
    d16 closes the recall, supersede, key-issuance and grant bodies only.

        Attributes:
            body (str): Text body — claim text, at most submission.max_body_size (shipped 1 MiB) UTF-8 bytes of the NFC-
                normalized body; larger is 413 request.payload_too_large (http-api.md Limits). Binary content goes through
                attachments.
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
            attachments (list[str] | Unset): At most 64 attachments per memory (http-api.md Limits). Each digest must name a
                completed upload session owned by this key.
            derives_from (list[str] | Unset): Artifact ids; the declared lineage (omission is undetectable — honesty rule).
            simulation_id (None | str | Unset): Required when derives_from touches a simulation row; key must be in the
                audience.
            simulated_time (datetime.datetime | None | Unset): Declared data, never backdating — the journal keeps the real
                timestamp.
            terms (list[str] | Unset): CURIE annotations (ADR-0021 d8): clear, non-erasable metadata entering the signed
                bytes; checked by the vocab scanner under the workarea's vocabulary_policy.
            term_status (SubmitRequestTermStatus | Unset): kind=term only. Structural payload field, never body text
                (ADR-0021 d1).
            successors (list[SubmitRequestSuccessorsItem] | Unset): kind=term with term_status deprecated only — required
                then: exactly one replaced_by, or one-or-more consider. The ratified oml:replaced_by / oml:consider claim edges
                fold from this declaration, atomically with the submission (ADR-0021 d3).
            mapping (SubmitRequestMapping | Unset): kind=term_mapping only — required then. Structural payload fields
                (ADR-0021 d6): the scanner's vocab.mapping_exactMatch routing and the mappings read surface depend on them, so
                they never ride erasable body bytes.
    """

    body: str
    verb: SubmitRequestVerb | Unset = UNSET
    claim_key: str | Unset = UNSET
    kind: SubmitRequestKind | Unset = SubmitRequestKind.MEMORY
    attachments: list[str] | Unset = UNSET
    derives_from: list[str] | Unset = UNSET
    simulation_id: None | str | Unset = UNSET
    simulated_time: datetime.datetime | None | Unset = UNSET
    terms: list[str] | Unset = UNSET
    term_status: SubmitRequestTermStatus | Unset = UNSET
    successors: list[SubmitRequestSuccessorsItem] | Unset = UNSET
    mapping: SubmitRequestMapping | Unset = UNSET
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

        attachments: list[str] | Unset = UNSET
        if not isinstance(self.attachments, Unset):
            attachments = self.attachments

        derives_from: list[str] | Unset = UNSET
        if not isinstance(self.derives_from, Unset):
            derives_from = self.derives_from

        simulation_id: None | str | Unset
        if isinstance(self.simulation_id, Unset):
            simulation_id = UNSET
        else:
            simulation_id = self.simulation_id

        simulated_time: None | str | Unset
        if isinstance(self.simulated_time, Unset):
            simulated_time = UNSET
        elif isinstance(self.simulated_time, datetime.datetime):
            simulated_time = self.simulated_time.isoformat()
        else:
            simulated_time = self.simulated_time

        terms: list[str] | Unset = UNSET
        if not isinstance(self.terms, Unset):
            terms = self.terms

        term_status: str | Unset = UNSET
        if not isinstance(self.term_status, Unset):
            term_status = self.term_status.value

        successors: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.successors, Unset):
            successors = []
            for successors_item_data in self.successors:
                successors_item = successors_item_data.to_dict()
                successors.append(successors_item)

        mapping: dict[str, Any] | Unset = UNSET
        if not isinstance(self.mapping, Unset):
            mapping = self.mapping.to_dict()

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
        if attachments is not UNSET:
            field_dict["attachments"] = attachments
        if derives_from is not UNSET:
            field_dict["derives_from"] = derives_from
        if simulation_id is not UNSET:
            field_dict["simulation_id"] = simulation_id
        if simulated_time is not UNSET:
            field_dict["simulated_time"] = simulated_time
        if terms is not UNSET:
            field_dict["terms"] = terms
        if term_status is not UNSET:
            field_dict["term_status"] = term_status
        if successors is not UNSET:
            field_dict["successors"] = successors
        if mapping is not UNSET:
            field_dict["mapping"] = mapping

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.submit_request_mapping import SubmitRequestMapping
        from ..models.submit_request_successors_item import SubmitRequestSuccessorsItem

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

        attachments = cast(list[str], d.pop("attachments", UNSET))

        derives_from = cast(list[str], d.pop("derives_from", UNSET))

        def _parse_simulation_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        simulation_id = _parse_simulation_id(d.pop("simulation_id", UNSET))

        def _parse_simulated_time(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                simulated_time_type_0 = datetime.datetime.fromisoformat(data)

                return simulated_time_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        simulated_time = _parse_simulated_time(d.pop("simulated_time", UNSET))

        terms = cast(list[str], d.pop("terms", UNSET))

        _term_status = d.pop("term_status", UNSET)
        term_status: SubmitRequestTermStatus | Unset
        if isinstance(_term_status, Unset):
            term_status = UNSET
        else:
            term_status = SubmitRequestTermStatus(_term_status)

        _successors = d.pop("successors", UNSET)
        successors: list[SubmitRequestSuccessorsItem] | Unset = UNSET
        if _successors is not UNSET:
            successors = []
            for successors_item_data in _successors:
                successors_item = SubmitRequestSuccessorsItem.from_dict(
                    successors_item_data
                )

                successors.append(successors_item)

        _mapping = d.pop("mapping", UNSET)
        mapping: SubmitRequestMapping | Unset
        if isinstance(_mapping, Unset):
            mapping = UNSET
        else:
            mapping = SubmitRequestMapping.from_dict(_mapping)

        submit_request = cls(
            body=body,
            verb=verb,
            claim_key=claim_key,
            kind=kind,
            attachments=attachments,
            derives_from=derives_from,
            simulation_id=simulation_id,
            simulated_time=simulated_time,
            terms=terms,
            term_status=term_status,
            successors=successors,
            mapping=mapping,
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
