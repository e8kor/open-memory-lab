from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.upload_open_request import UploadOpenRequest
from ...models.upload_session import UploadSession
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: UploadOpenRequest,
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
        "url": "/v2/uploads",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | UploadSession:
    if response.status_code == 201:
        response_201 = UploadSession.from_dict(response.json())

        return response_201

    if response.status_code == 400:
        response_400 = ErrorEnvelope.from_dict(response.json())

        return response_400

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

    if response.status_code == 413:
        response_413 = ErrorEnvelope.from_dict(response.json())

        return response_413

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    if response.status_code == 503:
        response_503 = ErrorEnvelope.from_dict(response.json())

        return response_503

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | UploadSession]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: UploadOpenRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | UploadSession]:
    """Open a resumable upload session for bytes of a declared size and SHA-256.

     Requires: a contributor or a submitter key; any other role is access.verb_ceiling (ADR-0004 d3, as
    amended 2026-11-17; ADR-0093 d6). purpose is import, the one purpose built; size is 1 byte to
    uploads.max_bytes (1 GiB by default), or 413 upload.too_large; sha256 is the plaintext's SHA-256 as
    64 lowercase hex digits. A key holding 4 sessions that are open, or completed and unbound, is
    upload.session_limit; a tenant whose open and unbound sessions would declare more than
    uploads.tenant_max_bytes (4 GiB by default) is upload.tenant_limit; an object store that fails is
    503 upload.storage_failed. The session takes parts of 8 MiB, the last the remainder, and expires 24
    hours after its opening or its last part and never later than 72 hours after it opened. Only this
    key is ever served it. Nothing is journalled, and no Idempotency-Key is taken: a repeated open opens
    another session. Since v0.8.3.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (UploadOpenRequest): An upload session to open (ADR-0004 d3, as amended 2026-11-17).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UploadSession]
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
    body: UploadOpenRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | UploadSession | None:
    """Open a resumable upload session for bytes of a declared size and SHA-256.

     Requires: a contributor or a submitter key; any other role is access.verb_ceiling (ADR-0004 d3, as
    amended 2026-11-17; ADR-0093 d6). purpose is import, the one purpose built; size is 1 byte to
    uploads.max_bytes (1 GiB by default), or 413 upload.too_large; sha256 is the plaintext's SHA-256 as
    64 lowercase hex digits. A key holding 4 sessions that are open, or completed and unbound, is
    upload.session_limit; a tenant whose open and unbound sessions would declare more than
    uploads.tenant_max_bytes (4 GiB by default) is upload.tenant_limit; an object store that fails is
    503 upload.storage_failed. The session takes parts of 8 MiB, the last the remainder, and expires 24
    hours after its opening or its last part and never later than 72 hours after it opened. Only this
    key is ever served it. Nothing is journalled, and no Idempotency-Key is taken: a repeated open opens
    another session. Since v0.8.3.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (UploadOpenRequest): An upload session to open (ADR-0004 d3, as amended 2026-11-17).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UploadSession
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
    body: UploadOpenRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | UploadSession]:
    """Open a resumable upload session for bytes of a declared size and SHA-256.

     Requires: a contributor or a submitter key; any other role is access.verb_ceiling (ADR-0004 d3, as
    amended 2026-11-17; ADR-0093 d6). purpose is import, the one purpose built; size is 1 byte to
    uploads.max_bytes (1 GiB by default), or 413 upload.too_large; sha256 is the plaintext's SHA-256 as
    64 lowercase hex digits. A key holding 4 sessions that are open, or completed and unbound, is
    upload.session_limit; a tenant whose open and unbound sessions would declare more than
    uploads.tenant_max_bytes (4 GiB by default) is upload.tenant_limit; an object store that fails is
    503 upload.storage_failed. The session takes parts of 8 MiB, the last the remainder, and expires 24
    hours after its opening or its last part and never later than 72 hours after it opened. Only this
    key is ever served it. Nothing is journalled, and no Idempotency-Key is taken: a repeated open opens
    another session. Since v0.8.3.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (UploadOpenRequest): An upload session to open (ADR-0004 d3, as amended 2026-11-17).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UploadSession]
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
    body: UploadOpenRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | UploadSession | None:
    """Open a resumable upload session for bytes of a declared size and SHA-256.

     Requires: a contributor or a submitter key; any other role is access.verb_ceiling (ADR-0004 d3, as
    amended 2026-11-17; ADR-0093 d6). purpose is import, the one purpose built; size is 1 byte to
    uploads.max_bytes (1 GiB by default), or 413 upload.too_large; sha256 is the plaintext's SHA-256 as
    64 lowercase hex digits. A key holding 4 sessions that are open, or completed and unbound, is
    upload.session_limit; a tenant whose open and unbound sessions would declare more than
    uploads.tenant_max_bytes (4 GiB by default) is upload.tenant_limit; an object store that fails is
    503 upload.storage_failed. The session takes parts of 8 MiB, the last the remainder, and expires 24
    hours after its opening or its last part and never later than 72 hours after it opened. Only this
    key is ever served it. Nothing is journalled, and no Idempotency-Key is taken: a repeated open opens
    another session. Since v0.8.3.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (UploadOpenRequest): An upload session to open (ADR-0004 d3, as amended 2026-11-17).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UploadSession
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
