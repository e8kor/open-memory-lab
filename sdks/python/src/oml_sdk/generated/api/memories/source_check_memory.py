from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.source_check_request import SourceCheckRequest
from ...models.source_check_result import SourceCheckResult
from ...types import UNSET, Response, Unset


def _get_kwargs(
    artifact_id: str,
    *,
    body: SourceCheckRequest,
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
        "url": "/v1/memories/{artifact_id}/source-check".format(
            artifact_id=quote(str(artifact_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | SourceCheckResult:
    if response.status_code == 200:
        response_200 = SourceCheckResult.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | SourceCheckResult]:
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
    body: SourceCheckRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | SourceCheckResult]:
    """Record that a promoted version's anchored span was compared with its source at a commit (ADR-0057
    d5, d18).

     Requires: the version's author key (writer), or a dreamer with dreamer effective access on its
    workarea; any other writer that can read the version is 403 access.author_only, and a key that
    cannot read it is 404 access.not_found. The version must be a promoted, anchored served tip: a held,
    unanchored or superseded one is request.validation, details.field /artifact_id. Journals
    source.checked {artifact_id, rev, result, hash, path, fragment}; changes no text and no anchor and
    moves nothing through review. The answer is the version's new stale and source_checked: stale is
    changed or removed from the latest check, else null, and it is computed from checks of served
    versions alone, never from a held one (R3). result moved names the span's new path (required) and
    fragment, and reads not stale; path and fragment are refused with any other result. Idempotency-Key
    required, scoped to the key with a nil workarea.

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SourceCheckRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SourceCheckResult]
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
    body: SourceCheckRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | SourceCheckResult | None:
    """Record that a promoted version's anchored span was compared with its source at a commit (ADR-0057
    d5, d18).

     Requires: the version's author key (writer), or a dreamer with dreamer effective access on its
    workarea; any other writer that can read the version is 403 access.author_only, and a key that
    cannot read it is 404 access.not_found. The version must be a promoted, anchored served tip: a held,
    unanchored or superseded one is request.validation, details.field /artifact_id. Journals
    source.checked {artifact_id, rev, result, hash, path, fragment}; changes no text and no anchor and
    moves nothing through review. The answer is the version's new stale and source_checked: stale is
    changed or removed from the latest check, else null, and it is computed from checks of served
    versions alone, never from a held one (R3). result moved names the span's new path (required) and
    fragment, and reads not stale; path and fragment are refused with any other result. Idempotency-Key
    required, scoped to the key with a nil workarea.

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SourceCheckRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SourceCheckResult
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
    body: SourceCheckRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | SourceCheckResult]:
    """Record that a promoted version's anchored span was compared with its source at a commit (ADR-0057
    d5, d18).

     Requires: the version's author key (writer), or a dreamer with dreamer effective access on its
    workarea; any other writer that can read the version is 403 access.author_only, and a key that
    cannot read it is 404 access.not_found. The version must be a promoted, anchored served tip: a held,
    unanchored or superseded one is request.validation, details.field /artifact_id. Journals
    source.checked {artifact_id, rev, result, hash, path, fragment}; changes no text and no anchor and
    moves nothing through review. The answer is the version's new stale and source_checked: stale is
    changed or removed from the latest check, else null, and it is computed from checks of served
    versions alone, never from a held one (R3). result moved names the span's new path (required) and
    fragment, and reads not stale; path and fragment are refused with any other result. Idempotency-Key
    required, scoped to the key with a nil workarea.

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SourceCheckRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SourceCheckResult]
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
    body: SourceCheckRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | SourceCheckResult | None:
    """Record that a promoted version's anchored span was compared with its source at a commit (ADR-0057
    d5, d18).

     Requires: the version's author key (writer), or a dreamer with dreamer effective access on its
    workarea; any other writer that can read the version is 403 access.author_only, and a key that
    cannot read it is 404 access.not_found. The version must be a promoted, anchored served tip: a held,
    unanchored or superseded one is request.validation, details.field /artifact_id. Journals
    source.checked {artifact_id, rev, result, hash, path, fragment}; changes no text and no anchor and
    moves nothing through review. The answer is the version's new stale and source_checked: stale is
    changed or removed from the latest check, else null, and it is computed from checks of served
    versions alone, never from a held one (R3). result moved names the span's new path (required) and
    fragment, and reads not stale; path and fragment are refused with any other result. Idempotency-Key
    required, scoped to the key with a nil workarea.

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SourceCheckRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SourceCheckResult
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
