from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.supersede_request_kind import SupersedeRequestKind
from ..models.supersede_request_term_status import SupersedeRequestTermStatus
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.supersede_request_mapping import SupersedeRequestMapping
    from ..models.supersede_request_successors_item import (
        SupersedeRequestSuccessorsItem,
    )


T = TypeVar("T", bound="SupersedeRequest")


@_attrs_define
class SupersedeRequest:
    """Unknown fields are refused (request.validation). The body declares every submission field a successor can need —
    simulation_id, simulated_time, mapping and the term fields — so closing it refuses nothing a supersession must carry
    (ADR-0025 d16).

        Attributes:
            body (str): Claim text, at most submission.max_body_size (shipped 1 MiB) UTF-8 bytes of the NFC-normalized body;
                larger is 413 request.payload_too_large (http-api.md Limits). With retirement true, why the claim is retired.
            retirement (bool | Unset): true proposes retiring the claim, and body states why. Refused with
                request.validation when the target's kind is term (terms deprecate), or when a kind is sent that differs from
                the target's. Default: False.
            kind (SupersedeRequestKind | Unset): Omitted, the successor takes the target's kind — the one field inherited
                from the target. A memory, derived_fact or steering target admits any of those three; a vocabulary-kind target
                and a retirement keep the target's kind; anything else is request.validation (docs/architecture/audit-and-
                lineage.md).
            attachments (list[str] | Unset): Omitted, the successor has no attachments; nothing is inherited from the
                target.
            derives_from (list[str] | Unset): The superseded target is included implicitly.
            reason (str | Unset):
            simulation_id (None | str | Unset): Required when the target or derives_from touches a simulation row
                (submission.simulation_id_required otherwise); the key must be in the audience. A tainted successor shadows the
                tip for its simulation audience only.
            simulated_time (datetime.datetime | None | Unset): Declared data, never backdating — the journal keeps the real
                timestamp.
            mapping (SupersedeRequestMapping | Unset): Successor of kind term_mapping only — required then, as on
                SubmitRequest. Structural payload fields (ADR-0021 d6).
            terms (list[str] | Unset): CURIE annotations (ADR-0021 d8). Omitted, the successor has none; nothing is
                inherited from the target.
            term_status (SupersedeRequestTermStatus | Unset): kind=term only — a deprecation IS a supersession with
                term_status deprecated (ADR-0021 d3).
            successors (list[SupersedeRequestSuccessorsItem] | Unset): Required when term_status is deprecated: exactly one
                replaced_by, or one-or-more consider — the successor rides the deprecating submission atomically; one serve and
                one verdict cover both (ADR-0021 d3).
    """

    body: str
    retirement: bool | Unset = False
    kind: SupersedeRequestKind | Unset = UNSET
    attachments: list[str] | Unset = UNSET
    derives_from: list[str] | Unset = UNSET
    reason: str | Unset = UNSET
    simulation_id: None | str | Unset = UNSET
    simulated_time: datetime.datetime | None | Unset = UNSET
    mapping: SupersedeRequestMapping | Unset = UNSET
    terms: list[str] | Unset = UNSET
    term_status: SupersedeRequestTermStatus | Unset = UNSET
    successors: list[SupersedeRequestSuccessorsItem] | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        body = self.body

        retirement = self.retirement

        kind: str | Unset = UNSET
        if not isinstance(self.kind, Unset):
            kind = self.kind.value

        attachments: list[str] | Unset = UNSET
        if not isinstance(self.attachments, Unset):
            attachments = self.attachments

        derives_from: list[str] | Unset = UNSET
        if not isinstance(self.derives_from, Unset):
            derives_from = self.derives_from

        reason = self.reason

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

        mapping: dict[str, Any] | Unset = UNSET
        if not isinstance(self.mapping, Unset):
            mapping = self.mapping.to_dict()

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

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "body": body,
            }
        )
        if retirement is not UNSET:
            field_dict["retirement"] = retirement
        if kind is not UNSET:
            field_dict["kind"] = kind
        if attachments is not UNSET:
            field_dict["attachments"] = attachments
        if derives_from is not UNSET:
            field_dict["derives_from"] = derives_from
        if reason is not UNSET:
            field_dict["reason"] = reason
        if simulation_id is not UNSET:
            field_dict["simulation_id"] = simulation_id
        if simulated_time is not UNSET:
            field_dict["simulated_time"] = simulated_time
        if mapping is not UNSET:
            field_dict["mapping"] = mapping
        if terms is not UNSET:
            field_dict["terms"] = terms
        if term_status is not UNSET:
            field_dict["term_status"] = term_status
        if successors is not UNSET:
            field_dict["successors"] = successors

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.supersede_request_mapping import SupersedeRequestMapping
        from ..models.supersede_request_successors_item import (
            SupersedeRequestSuccessorsItem,
        )

        d = dict(src_dict)
        body = d.pop("body")

        retirement = d.pop("retirement", UNSET)

        _kind = d.pop("kind", UNSET)
        kind: SupersedeRequestKind | Unset
        if isinstance(_kind, Unset):
            kind = UNSET
        else:
            kind = SupersedeRequestKind(_kind)

        attachments = cast(list[str], d.pop("attachments", UNSET))

        derives_from = cast(list[str], d.pop("derives_from", UNSET))

        reason = d.pop("reason", UNSET)

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

        _mapping = d.pop("mapping", UNSET)
        mapping: SupersedeRequestMapping | Unset
        if isinstance(_mapping, Unset):
            mapping = UNSET
        else:
            mapping = SupersedeRequestMapping.from_dict(_mapping)

        terms = cast(list[str], d.pop("terms", UNSET))

        _term_status = d.pop("term_status", UNSET)
        term_status: SupersedeRequestTermStatus | Unset
        if isinstance(_term_status, Unset):
            term_status = UNSET
        else:
            term_status = SupersedeRequestTermStatus(_term_status)

        _successors = d.pop("successors", UNSET)
        successors: list[SupersedeRequestSuccessorsItem] | Unset = UNSET
        if _successors is not UNSET:
            successors = []
            for successors_item_data in _successors:
                successors_item = SupersedeRequestSuccessorsItem.from_dict(
                    successors_item_data
                )

                successors.append(successors_item)

        supersede_request = cls(
            body=body,
            retirement=retirement,
            kind=kind,
            attachments=attachments,
            derives_from=derives_from,
            reason=reason,
            simulation_id=simulation_id,
            simulated_time=simulated_time,
            mapping=mapping,
            terms=terms,
            term_status=term_status,
            successors=successors,
        )

        return supersede_request
