from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.upload_session import UploadSession
from ...types import UNSET, Response, Unset


def _get_kwargs(
    upload_id: str,
    *,
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
        "url": "/v2/uploads/{upload_id}/complete".format(
            upload_id=quote(str(upload_id), safe=""),
        ),
    }

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
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | UploadSession]:
    """Read the parts in order and check their SHA-256 against the declared one.

     Requires: the key that opened the session, or access.not_found (ADR-0004 d3, as amended 2026-11-17).
    Parts missing are upload.incomplete with details.missing, the first 100. A digest that differs fails
    the session, nulls its key, removes its parts and answers 422 upload.digest_mismatch. A match
    completes it: it is unbound until a verb such as an import binds it, and expires 24 hours after
    completion. A session that is not open is upload.session_invalid; past its expiry, 410
    upload.expired. Nothing is journalled. Since v0.8.3.

    Args:
        upload_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UploadSession]
    """

    kwargs = _get_kwargs(
        upload_id=upload_id,
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
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | UploadSession | None:
    """Read the parts in order and check their SHA-256 against the declared one.

     Requires: the key that opened the session, or access.not_found (ADR-0004 d3, as amended 2026-11-17).
    Parts missing are upload.incomplete with details.missing, the first 100. A digest that differs fails
    the session, nulls its key, removes its parts and answers 422 upload.digest_mismatch. A match
    completes it: it is unbound until a verb such as an import binds it, and expires 24 hours after
    completion. A session that is not open is upload.session_invalid; past its expiry, 410
    upload.expired. Nothing is journalled. Since v0.8.3.

    Args:
        upload_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UploadSession
    """

    return sync_detailed(
        upload_id=upload_id,
        client=client,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    upload_id: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | UploadSession]:
    """Read the parts in order and check their SHA-256 against the declared one.

     Requires: the key that opened the session, or access.not_found (ADR-0004 d3, as amended 2026-11-17).
    Parts missing are upload.incomplete with details.missing, the first 100. A digest that differs fails
    the session, nulls its key, removes its parts and answers 422 upload.digest_mismatch. A match
    completes it: it is unbound until a verb such as an import binds it, and expires 24 hours after
    completion. A session that is not open is upload.session_invalid; past its expiry, 410
    upload.expired. Nothing is journalled. Since v0.8.3.

    Args:
        upload_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UploadSession]
    """

    kwargs = _get_kwargs(
        upload_id=upload_id,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    upload_id: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | UploadSession | None:
    """Read the parts in order and check their SHA-256 against the declared one.

     Requires: the key that opened the session, or access.not_found (ADR-0004 d3, as amended 2026-11-17).
    Parts missing are upload.incomplete with details.missing, the first 100. A digest that differs fails
    the session, nulls its key, removes its parts and answers 422 upload.digest_mismatch. A match
    completes it: it is unbound until a verb such as an import binds it, and expires 24 hours after
    completion. A session that is not open is upload.session_invalid; past its expiry, 410
    upload.expired. Nothing is journalled. Since v0.8.3.

    Args:
        upload_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UploadSession
    """

    return (
        await asyncio_detailed(
            upload_id=upload_id,
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
