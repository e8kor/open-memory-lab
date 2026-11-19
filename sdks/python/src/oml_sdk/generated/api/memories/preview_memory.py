from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.memory_preview import MemoryPreview
from ...models.preview_supersede_request import PreviewSupersedeRequest
from ...models.submit_request import SubmitRequest
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: PreviewSupersedeRequest | SubmitRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/memories/preview",
    }

    if isinstance(body, SubmitRequest):
        _kwargs["json"] = body.to_dict()
    else:
        _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | MemoryPreview:
    if response.status_code == 200:
        response_200 = MemoryPreview.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = ErrorEnvelope.from_dict(response.json())

        return response_401

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    if response.status_code == 429:
        response_429 = ErrorEnvelope.from_dict(response.json())

        return response_429

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | MemoryPreview]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: PreviewSupersedeRequest | SubmitRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | MemoryPreview]:
    """What capture would decide for a submit's body or a supersession's, without capturing.

     Requires: the grant submit would need, or, with replaces, the grant supersede would need on that
    target (ADR-0082 d5). The body is submit's, or supersede's with replaces naming the version it
    replaces. Every check capture makes runs through the same pipeline, which stops before anything is
    journalled, held or metered: no version, no event, no counter moves (ADR-0071 d2), and no
    Idempotency-Key is read, so nothing is recorded or replayed. A refusal of authority is the HTTP
    error submit or supersede would answer: auth.*, limit.rate, access.verb_ceiling, every access.*
    grant refusal, and access.not_found for a target the key cannot read, replaces that names no row
    included. Every other refusal capture would make, the body's shape, size, the scan's veto, a quota's
    headroom, a name taken, submission.version_conflict, submission.tip_conflict, workspace.frozen and a
    guardrail rule that does not parse among them, answers 200 with would_capture false and refusal in
    the error object's shape, beside what was read before it (ADR-0082 d10). The request rate is charged
    once (ADR-0079). Read-shaped POST. Since v0.8.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (PreviewSupersedeRequest | SubmitRequest): Submit's body, or supersede's with
            replaces naming the version it would replace (ADR-0082 d5).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | MemoryPreview]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: PreviewSupersedeRequest | SubmitRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | MemoryPreview | None:
    """What capture would decide for a submit's body or a supersession's, without capturing.

     Requires: the grant submit would need, or, with replaces, the grant supersede would need on that
    target (ADR-0082 d5). The body is submit's, or supersede's with replaces naming the version it
    replaces. Every check capture makes runs through the same pipeline, which stops before anything is
    journalled, held or metered: no version, no event, no counter moves (ADR-0071 d2), and no
    Idempotency-Key is read, so nothing is recorded or replayed. A refusal of authority is the HTTP
    error submit or supersede would answer: auth.*, limit.rate, access.verb_ceiling, every access.*
    grant refusal, and access.not_found for a target the key cannot read, replaces that names no row
    included. Every other refusal capture would make, the body's shape, size, the scan's veto, a quota's
    headroom, a name taken, submission.version_conflict, submission.tip_conflict, workspace.frozen and a
    guardrail rule that does not parse among them, answers 200 with would_capture false and refusal in
    the error object's shape, beside what was read before it (ADR-0082 d10). The request rate is charged
    once (ADR-0079). Read-shaped POST. Since v0.8.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (PreviewSupersedeRequest | SubmitRequest): Submit's body, or supersede's with
            replaces naming the version it would replace (ADR-0082 d5).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | MemoryPreview
    """

    return sync_detailed(
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: PreviewSupersedeRequest | SubmitRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | MemoryPreview]:
    """What capture would decide for a submit's body or a supersession's, without capturing.

     Requires: the grant submit would need, or, with replaces, the grant supersede would need on that
    target (ADR-0082 d5). The body is submit's, or supersede's with replaces naming the version it
    replaces. Every check capture makes runs through the same pipeline, which stops before anything is
    journalled, held or metered: no version, no event, no counter moves (ADR-0071 d2), and no
    Idempotency-Key is read, so nothing is recorded or replayed. A refusal of authority is the HTTP
    error submit or supersede would answer: auth.*, limit.rate, access.verb_ceiling, every access.*
    grant refusal, and access.not_found for a target the key cannot read, replaces that names no row
    included. Every other refusal capture would make, the body's shape, size, the scan's veto, a quota's
    headroom, a name taken, submission.version_conflict, submission.tip_conflict, workspace.frozen and a
    guardrail rule that does not parse among them, answers 200 with would_capture false and refusal in
    the error object's shape, beside what was read before it (ADR-0082 d10). The request rate is charged
    once (ADR-0079). Read-shaped POST. Since v0.8.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (PreviewSupersedeRequest | SubmitRequest): Submit's body, or supersede's with
            replaces naming the version it would replace (ADR-0082 d5).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | MemoryPreview]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: PreviewSupersedeRequest | SubmitRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | MemoryPreview | None:
    """What capture would decide for a submit's body or a supersession's, without capturing.

     Requires: the grant submit would need, or, with replaces, the grant supersede would need on that
    target (ADR-0082 d5). The body is submit's, or supersede's with replaces naming the version it
    replaces. Every check capture makes runs through the same pipeline, which stops before anything is
    journalled, held or metered: no version, no event, no counter moves (ADR-0071 d2), and no
    Idempotency-Key is read, so nothing is recorded or replayed. A refusal of authority is the HTTP
    error submit or supersede would answer: auth.*, limit.rate, access.verb_ceiling, every access.*
    grant refusal, and access.not_found for a target the key cannot read, replaces that names no row
    included. Every other refusal capture would make, the body's shape, size, the scan's veto, a quota's
    headroom, a name taken, submission.version_conflict, submission.tip_conflict, workspace.frozen and a
    guardrail rule that does not parse among them, answers 200 with would_capture false and refusal in
    the error object's shape, beside what was read before it (ADR-0082 d10). The request rate is charged
    once (ADR-0079). Read-shaped POST. Since v0.8.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (PreviewSupersedeRequest | SubmitRequest): Submit's body, or supersede's with
            replaces naming the version it would replace (ADR-0082 d5).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | MemoryPreview
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
