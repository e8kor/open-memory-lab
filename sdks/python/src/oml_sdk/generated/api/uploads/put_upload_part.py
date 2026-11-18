from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.upload_session import UploadSession
from ...types import UNSET, File, Response, Unset


def _get_kwargs(
    upload_id: str,
    part: int,
    *,
    body: File,
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
        "method": "put",
        "url": "/v2/uploads/{upload_id}/parts/{part}".format(
            upload_id=quote(str(upload_id), safe=""),
            part=quote(str(part), safe=""),
        ),
    }

    _kwargs["content"] = body.payload
    headers["Content-Type"] = "application/octet-stream"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | UploadSession:
    if response.status_code == 200:
        response_200 = UploadSession.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

    if response.status_code == 410:
        response_410 = ErrorEnvelope.from_dict(response.json())

        return response_410

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    if response.status_code == 422:
        response_422 = ErrorEnvelope.from_dict(response.json())

        return response_422

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
    upload_id: str,
    part: int,
    *,
    client: AuthenticatedClient | Client,
    body: File,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | UploadSession]:
    """Store one part of an open session; sending a part again replaces it.

     Requires: the key that opened the session, or access.not_found (ADR-0004 d3, as amended 2026-11-17).
    part counts from 1; outside 1 to the part count is 422 upload.part_number. Every part is 8,388,608
    bytes but the last, which is the remainder; another length is 422 upload.part_size. A session that
    is not open is upload.session_invalid with details.state, and one past its expiry 410
    upload.expired. The part is stored as XChaCha20-Poly1305 ciphertext under the session's own key, and
    the session's expiry moves to 24 hours on, never past 72 hours after it opened. A part is addressed
    by its number, so no Idempotency-Key is taken: a retry after an interruption sends the parts GET
    /v2/uploads/{upload_id} names missing. Since v0.8.2.

    Args:
        upload_id (str):
        part (int):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (File):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UploadSession]
    """

    kwargs = _get_kwargs(
        upload_id=upload_id,
        part=part,
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
    upload_id: str,
    part: int,
    *,
    client: AuthenticatedClient | Client,
    body: File,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | UploadSession | None:
    """Store one part of an open session; sending a part again replaces it.

     Requires: the key that opened the session, or access.not_found (ADR-0004 d3, as amended 2026-11-17).
    part counts from 1; outside 1 to the part count is 422 upload.part_number. Every part is 8,388,608
    bytes but the last, which is the remainder; another length is 422 upload.part_size. A session that
    is not open is upload.session_invalid with details.state, and one past its expiry 410
    upload.expired. The part is stored as XChaCha20-Poly1305 ciphertext under the session's own key, and
    the session's expiry moves to 24 hours on, never past 72 hours after it opened. A part is addressed
    by its number, so no Idempotency-Key is taken: a retry after an interruption sends the parts GET
    /v2/uploads/{upload_id} names missing. Since v0.8.2.

    Args:
        upload_id (str):
        part (int):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (File):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UploadSession
    """

    return sync_detailed(
        upload_id=upload_id,
        part=part,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    upload_id: str,
    part: int,
    *,
    client: AuthenticatedClient | Client,
    body: File,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | UploadSession]:
    """Store one part of an open session; sending a part again replaces it.

     Requires: the key that opened the session, or access.not_found (ADR-0004 d3, as amended 2026-11-17).
    part counts from 1; outside 1 to the part count is 422 upload.part_number. Every part is 8,388,608
    bytes but the last, which is the remainder; another length is 422 upload.part_size. A session that
    is not open is upload.session_invalid with details.state, and one past its expiry 410
    upload.expired. The part is stored as XChaCha20-Poly1305 ciphertext under the session's own key, and
    the session's expiry moves to 24 hours on, never past 72 hours after it opened. A part is addressed
    by its number, so no Idempotency-Key is taken: a retry after an interruption sends the parts GET
    /v2/uploads/{upload_id} names missing. Since v0.8.2.

    Args:
        upload_id (str):
        part (int):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (File):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UploadSession]
    """

    kwargs = _get_kwargs(
        upload_id=upload_id,
        part=part,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    upload_id: str,
    part: int,
    *,
    client: AuthenticatedClient | Client,
    body: File,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | UploadSession | None:
    """Store one part of an open session; sending a part again replaces it.

     Requires: the key that opened the session, or access.not_found (ADR-0004 d3, as amended 2026-11-17).
    part counts from 1; outside 1 to the part count is 422 upload.part_number. Every part is 8,388,608
    bytes but the last, which is the remainder; another length is 422 upload.part_size. A session that
    is not open is upload.session_invalid with details.state, and one past its expiry 410
    upload.expired. The part is stored as XChaCha20-Poly1305 ciphertext under the session's own key, and
    the session's expiry moves to 24 hours on, never past 72 hours after it opened. A part is addressed
    by its number, so no Idempotency-Key is taken: a retry after an interruption sends the parts GET
    /v2/uploads/{upload_id} names missing. Since v0.8.2.

    Args:
        upload_id (str):
        part (int):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (File):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UploadSession
    """

    return (
        await asyncio_detailed(
            upload_id=upload_id,
            part=part,
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
