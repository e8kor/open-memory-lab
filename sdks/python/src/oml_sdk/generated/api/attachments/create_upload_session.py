from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.upload_session import UploadSession
from ...models.upload_session_create import UploadSessionCreate
from ...types import Response


def _get_kwargs(
    *,
    body: UploadSessionCreate,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/attachments/uploads",
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
    body: UploadSessionCreate,
    idempotency_key: str,
) -> Response[ErrorEnvelope | UploadSession]:
    """Open a resumable upload session recording the expected plaintext digest up front.

     Requires: writer effective access. The session is scoped to the uploading key and expires (D3).

    Args:
        idempotency_key (str):
        body (UploadSessionCreate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UploadSession]
    """

    kwargs = _get_kwargs(
        body=body,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: UploadSessionCreate,
    idempotency_key: str,
) -> ErrorEnvelope | UploadSession | None:
    """Open a resumable upload session recording the expected plaintext digest up front.

     Requires: writer effective access. The session is scoped to the uploading key and expires (D3).

    Args:
        idempotency_key (str):
        body (UploadSessionCreate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UploadSession
    """

    return sync_detailed(
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: UploadSessionCreate,
    idempotency_key: str,
) -> Response[ErrorEnvelope | UploadSession]:
    """Open a resumable upload session recording the expected plaintext digest up front.

     Requires: writer effective access. The session is scoped to the uploading key and expires (D3).

    Args:
        idempotency_key (str):
        body (UploadSessionCreate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UploadSession]
    """

    kwargs = _get_kwargs(
        body=body,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: UploadSessionCreate,
    idempotency_key: str,
) -> ErrorEnvelope | UploadSession | None:
    """Open a resumable upload session recording the expected plaintext digest up front.

     Requires: writer effective access. The session is scoped to the uploading key and expires (D3).

    Args:
        idempotency_key (str):
        body (UploadSessionCreate):

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
            idempotency_key=idempotency_key,
        )
    ).parsed
