from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.upload_complete import UploadComplete
from ...types import Response


def _get_kwargs(
    session_id: str,
    *,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/attachments/uploads/{session_id}/complete".format(
            session_id=quote(str(session_id), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | UploadComplete:
    if response.status_code == 200:
        response_200 = UploadComplete.from_dict(response.json())

        return response_200

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

    if response.status_code == 422:
        response_422 = ErrorEnvelope.from_dict(response.json())

        return response_422

    if response.status_code == 429:
        response_429 = ErrorEnvelope.from_dict(response.json())

        return response_429

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | UploadComplete]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    session_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> Response[ErrorEnvelope | UploadComplete]:
    """Assemble, hash, compare against the expected digest; store encrypted on match.

     Requires: writer, session owner. upload.digest_mismatch when the bytes are not what the session
    promised.

    Args:
        session_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UploadComplete]
    """

    kwargs = _get_kwargs(
        session_id=session_id,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    session_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> ErrorEnvelope | UploadComplete | None:
    """Assemble, hash, compare against the expected digest; store encrypted on match.

     Requires: writer, session owner. upload.digest_mismatch when the bytes are not what the session
    promised.

    Args:
        session_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UploadComplete
    """

    return sync_detailed(
        session_id=session_id,
        client=client,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    session_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> Response[ErrorEnvelope | UploadComplete]:
    """Assemble, hash, compare against the expected digest; store encrypted on match.

     Requires: writer, session owner. upload.digest_mismatch when the bytes are not what the session
    promised.

    Args:
        session_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UploadComplete]
    """

    kwargs = _get_kwargs(
        session_id=session_id,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    session_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> ErrorEnvelope | UploadComplete | None:
    """Assemble, hash, compare against the expected digest; store encrypted on match.

     Requires: writer, session owner. upload.digest_mismatch when the bytes are not what the session
    promised.

    Args:
        session_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UploadComplete
    """

    return (
        await asyncio_detailed(
            session_id=session_id,
            client=client,
            idempotency_key=idempotency_key,
        )
    ).parsed
