from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...types import File, Response


def _get_kwargs(
    session_id: str,
    part_number: int,
    *,
    body: File,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "put",
        "url": "/v1/attachments/uploads/{session_id}/parts/{part_number}".format(
            session_id=quote(str(session_id), safe=""),
            part_number=quote(str(part_number), safe=""),
        ),
    }

    _kwargs["content"] = body.payload
    headers["Content-Type"] = "application/octet-stream"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | ErrorEnvelope:
    if response.status_code == 204:
        response_204 = cast(Any, None)
        return response_204

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

    if response.status_code == 413:
        response_413 = ErrorEnvelope.from_dict(response.json())

        return response_413

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
) -> Response[Any | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    session_id: str,
    part_number: int,
    *,
    client: AuthenticatedClient | Client,
    body: File,
) -> Response[Any | ErrorEnvelope]:
    """Upload one part; replaying a part is harmless (verified by digest at completion).

     Requires: writer, session owner. No Idempotency-Key — a part is addressed by (session, part_number).
    Parts are 5–64 MiB (http-api.md Limits); an oversized part answers 413 request.payload_too_large.

    Args:
        session_id (str):
        part_number (int):
        body (File):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        session_id=session_id,
        part_number=part_number,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    session_id: str,
    part_number: int,
    *,
    client: AuthenticatedClient | Client,
    body: File,
) -> Any | ErrorEnvelope | None:
    """Upload one part; replaying a part is harmless (verified by digest at completion).

     Requires: writer, session owner. No Idempotency-Key — a part is addressed by (session, part_number).
    Parts are 5–64 MiB (http-api.md Limits); an oversized part answers 413 request.payload_too_large.

    Args:
        session_id (str):
        part_number (int):
        body (File):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ErrorEnvelope
    """

    return sync_detailed(
        session_id=session_id,
        part_number=part_number,
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    session_id: str,
    part_number: int,
    *,
    client: AuthenticatedClient | Client,
    body: File,
) -> Response[Any | ErrorEnvelope]:
    """Upload one part; replaying a part is harmless (verified by digest at completion).

     Requires: writer, session owner. No Idempotency-Key — a part is addressed by (session, part_number).
    Parts are 5–64 MiB (http-api.md Limits); an oversized part answers 413 request.payload_too_large.

    Args:
        session_id (str):
        part_number (int):
        body (File):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        session_id=session_id,
        part_number=part_number,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    session_id: str,
    part_number: int,
    *,
    client: AuthenticatedClient | Client,
    body: File,
) -> Any | ErrorEnvelope | None:
    """Upload one part; replaying a part is harmless (verified by digest at completion).

     Requires: writer, session owner. No Idempotency-Key — a part is addressed by (session, part_number).
    Parts are 5–64 MiB (http-api.md Limits); an oversized part answers 413 request.payload_too_large.

    Args:
        session_id (str):
        part_number (int):
        body (File):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            session_id=session_id,
            part_number=part_number,
            client=client,
            body=body,
        )
    ).parsed
