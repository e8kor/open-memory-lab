from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.submit_accepted import SubmitAccepted
from ...models.supersede_request import SupersedeRequest
from ...types import UNSET, Response, Unset


def _get_kwargs(
    artifact_id: str,
    *,
    body: SupersedeRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/memories/{artifact_id}/supersede".format(
            artifact_id=quote(str(artifact_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | SubmitAccepted:
    if response.status_code == 202:
        response_202 = SubmitAccepted.from_dict(response.json())

        return response_202

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | SubmitAccepted]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: SupersedeRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | SubmitAccepted]:
    """Propose a replacement version, or with retirement true propose retiring the claim; a new attributed
    submission, held like any write.

     Requires: the supersede rule declared in docs/architecture/roles-and-keys.md (Grant admissibility
    and refusal order) — a writer on a target version it authored, a dreamer through edit-as-proposal. A
    target that is a ratified retirement in a user's root takes a successor, which would put the claim
    back in service, from that user's keys alone: any other key that reads the target is 409
    submission.user_root_refused, answered after step 2 of that rule and ahead of access.author_only and
    access.grant_refused; every other successor there is under the supersede rule (ADR-0044 d16). The
    target must be the current served tip: submission.tip_conflict, with details.current_tip when the
    caller can read it. The tip moves only at ratification, by compare-and-set; the tip, version-
    ordinal, competing-successor, retirement and retired-claim rules are declared in
    docs/architecture/audit-and-lineage.md (ADR-0025 d10, d11). The body's workarea, if sent, must equal
    the target's workarea (request.validation, details.field /workarea, otherwise), and an X-OML-
    Workarea header is request.validation naming /workarea in details.renamed_to (ADR-0039 d7); that
    check, the tip, the kind and retirement checks and workarea.frozen are answered only after the
    target is known readable, in the order of step 5 of the supersede rule. Idempotency-Key is scoped to
    the key with a nil workarea; the record is looked up after step 1 of that rule and before step 2, so
    a committed supersession replays its stored response whatever the target's state or the key's access
    now, and it is claimed only after step 2 passes. Only kind is inherited from the target; a successor
    declares its own anchor or has none (ADR-0057 d2). expected_version is required: the claim's latest
    version the caller can read (its promoted versions where the caller reads the workarea, and the
    versions its own keys wrote; never another author's held version) as the caller last read it; any
    other is 409 submission.version_conflict with details.current_version, answered after the target is
    known readable and before the tip check, with nothing captured (ADR-0057 d19). The designed
    attachment, simulation and vocabulary fields of a successor are declared in openapi-designed.yaml
    (ADR-0039 d9).

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SupersedeRequest): Unknown fields are refused (request.validation; ADR-0025 d16).
            The designed successor fields — attachments, simulation_id, simulated_time, mapping and
            the term fields — are declared under x-oml-designed-fields in openapi-designed.yaml, and
            each sent here answers its v0.3.0 code rather than the unknown-field refusal (ADR-0039
            d9).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SubmitAccepted]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: SupersedeRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | SubmitAccepted | None:
    """Propose a replacement version, or with retirement true propose retiring the claim; a new attributed
    submission, held like any write.

     Requires: the supersede rule declared in docs/architecture/roles-and-keys.md (Grant admissibility
    and refusal order) — a writer on a target version it authored, a dreamer through edit-as-proposal. A
    target that is a ratified retirement in a user's root takes a successor, which would put the claim
    back in service, from that user's keys alone: any other key that reads the target is 409
    submission.user_root_refused, answered after step 2 of that rule and ahead of access.author_only and
    access.grant_refused; every other successor there is under the supersede rule (ADR-0044 d16). The
    target must be the current served tip: submission.tip_conflict, with details.current_tip when the
    caller can read it. The tip moves only at ratification, by compare-and-set; the tip, version-
    ordinal, competing-successor, retirement and retired-claim rules are declared in
    docs/architecture/audit-and-lineage.md (ADR-0025 d10, d11). The body's workarea, if sent, must equal
    the target's workarea (request.validation, details.field /workarea, otherwise), and an X-OML-
    Workarea header is request.validation naming /workarea in details.renamed_to (ADR-0039 d7); that
    check, the tip, the kind and retirement checks and workarea.frozen are answered only after the
    target is known readable, in the order of step 5 of the supersede rule. Idempotency-Key is scoped to
    the key with a nil workarea; the record is looked up after step 1 of that rule and before step 2, so
    a committed supersession replays its stored response whatever the target's state or the key's access
    now, and it is claimed only after step 2 passes. Only kind is inherited from the target; a successor
    declares its own anchor or has none (ADR-0057 d2). expected_version is required: the claim's latest
    version the caller can read (its promoted versions where the caller reads the workarea, and the
    versions its own keys wrote; never another author's held version) as the caller last read it; any
    other is 409 submission.version_conflict with details.current_version, answered after the target is
    known readable and before the tip check, with nothing captured (ADR-0057 d19). The designed
    attachment, simulation and vocabulary fields of a successor are declared in openapi-designed.yaml
    (ADR-0039 d9).

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SupersedeRequest): Unknown fields are refused (request.validation; ADR-0025 d16).
            The designed successor fields — attachments, simulation_id, simulated_time, mapping and
            the term fields — are declared under x-oml-designed-fields in openapi-designed.yaml, and
            each sent here answers its v0.3.0 code rather than the unknown-field refusal (ADR-0039
            d9).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SubmitAccepted
    """

    return sync_detailed(
        artifact_id=artifact_id,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: SupersedeRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | SubmitAccepted]:
    """Propose a replacement version, or with retirement true propose retiring the claim; a new attributed
    submission, held like any write.

     Requires: the supersede rule declared in docs/architecture/roles-and-keys.md (Grant admissibility
    and refusal order) — a writer on a target version it authored, a dreamer through edit-as-proposal. A
    target that is a ratified retirement in a user's root takes a successor, which would put the claim
    back in service, from that user's keys alone: any other key that reads the target is 409
    submission.user_root_refused, answered after step 2 of that rule and ahead of access.author_only and
    access.grant_refused; every other successor there is under the supersede rule (ADR-0044 d16). The
    target must be the current served tip: submission.tip_conflict, with details.current_tip when the
    caller can read it. The tip moves only at ratification, by compare-and-set; the tip, version-
    ordinal, competing-successor, retirement and retired-claim rules are declared in
    docs/architecture/audit-and-lineage.md (ADR-0025 d10, d11). The body's workarea, if sent, must equal
    the target's workarea (request.validation, details.field /workarea, otherwise), and an X-OML-
    Workarea header is request.validation naming /workarea in details.renamed_to (ADR-0039 d7); that
    check, the tip, the kind and retirement checks and workarea.frozen are answered only after the
    target is known readable, in the order of step 5 of the supersede rule. Idempotency-Key is scoped to
    the key with a nil workarea; the record is looked up after step 1 of that rule and before step 2, so
    a committed supersession replays its stored response whatever the target's state or the key's access
    now, and it is claimed only after step 2 passes. Only kind is inherited from the target; a successor
    declares its own anchor or has none (ADR-0057 d2). expected_version is required: the claim's latest
    version the caller can read (its promoted versions where the caller reads the workarea, and the
    versions its own keys wrote; never another author's held version) as the caller last read it; any
    other is 409 submission.version_conflict with details.current_version, answered after the target is
    known readable and before the tip check, with nothing captured (ADR-0057 d19). The designed
    attachment, simulation and vocabulary fields of a successor are declared in openapi-designed.yaml
    (ADR-0039 d9).

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SupersedeRequest): Unknown fields are refused (request.validation; ADR-0025 d16).
            The designed successor fields — attachments, simulation_id, simulated_time, mapping and
            the term fields — are declared under x-oml-designed-fields in openapi-designed.yaml, and
            each sent here answers its v0.3.0 code rather than the unknown-field refusal (ADR-0039
            d9).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SubmitAccepted]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: SupersedeRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | SubmitAccepted | None:
    """Propose a replacement version, or with retirement true propose retiring the claim; a new attributed
    submission, held like any write.

     Requires: the supersede rule declared in docs/architecture/roles-and-keys.md (Grant admissibility
    and refusal order) — a writer on a target version it authored, a dreamer through edit-as-proposal. A
    target that is a ratified retirement in a user's root takes a successor, which would put the claim
    back in service, from that user's keys alone: any other key that reads the target is 409
    submission.user_root_refused, answered after step 2 of that rule and ahead of access.author_only and
    access.grant_refused; every other successor there is under the supersede rule (ADR-0044 d16). The
    target must be the current served tip: submission.tip_conflict, with details.current_tip when the
    caller can read it. The tip moves only at ratification, by compare-and-set; the tip, version-
    ordinal, competing-successor, retirement and retired-claim rules are declared in
    docs/architecture/audit-and-lineage.md (ADR-0025 d10, d11). The body's workarea, if sent, must equal
    the target's workarea (request.validation, details.field /workarea, otherwise), and an X-OML-
    Workarea header is request.validation naming /workarea in details.renamed_to (ADR-0039 d7); that
    check, the tip, the kind and retirement checks and workarea.frozen are answered only after the
    target is known readable, in the order of step 5 of the supersede rule. Idempotency-Key is scoped to
    the key with a nil workarea; the record is looked up after step 1 of that rule and before step 2, so
    a committed supersession replays its stored response whatever the target's state or the key's access
    now, and it is claimed only after step 2 passes. Only kind is inherited from the target; a successor
    declares its own anchor or has none (ADR-0057 d2). expected_version is required: the claim's latest
    version the caller can read (its promoted versions where the caller reads the workarea, and the
    versions its own keys wrote; never another author's held version) as the caller last read it; any
    other is 409 submission.version_conflict with details.current_version, answered after the target is
    known readable and before the tip check, with nothing captured (ADR-0057 d19). The designed
    attachment, simulation and vocabulary fields of a successor are declared in openapi-designed.yaml
    (ADR-0039 d9).

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SupersedeRequest): Unknown fields are refused (request.validation; ADR-0025 d16).
            The designed successor fields — attachments, simulation_id, simulated_time, mapping and
            the term fields — are declared under x-oml-designed-fields in openapi-designed.yaml, and
            each sent here answers its v0.3.0 code rather than the unknown-field refusal (ADR-0039
            d9).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SubmitAccepted
    """

    return (
        await asyncio_detailed(
            artifact_id=artifact_id,
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            idempotency_key=idempotency_key,
        )
    ).parsed
