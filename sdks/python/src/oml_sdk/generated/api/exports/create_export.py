from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.export_job import ExportJob
from ...models.export_request import ExportRequest
from ...types import Response


def _get_kwargs(
    *,
    body: ExportRequest,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/exports",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | ExportJob:
    if response.status_code == 202:
        response_202 = ExportJob.from_dict(response.json())

        return response_202

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | ExportJob]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: ExportRequest,
    idempotency_key: str,
) -> Response[ErrorEnvelope | ExportJob]:
    """Start an async export job — the signed evidence bundle, or the digest-only chain stream for client-
    side verification.

     Requires: admin key role. The export act is itself journalled; simulations are excluded unless asked
    for and then listed separately.

    Args:
        idempotency_key (str):
        body (ExportRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ExportJob]
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
    body: ExportRequest,
    idempotency_key: str,
) -> ErrorEnvelope | ExportJob | None:
    """Start an async export job — the signed evidence bundle, or the digest-only chain stream for client-
    side verification.

     Requires: admin key role. The export act is itself journalled; simulations are excluded unless asked
    for and then listed separately.

    Args:
        idempotency_key (str):
        body (ExportRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ExportJob
    """

    return sync_detailed(
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: ExportRequest,
    idempotency_key: str,
) -> Response[ErrorEnvelope | ExportJob]:
    """Start an async export job — the signed evidence bundle, or the digest-only chain stream for client-
    side verification.

     Requires: admin key role. The export act is itself journalled; simulations are excluded unless asked
    for and then listed separately.

    Args:
        idempotency_key (str):
        body (ExportRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ExportJob]
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
    body: ExportRequest,
    idempotency_key: str,
) -> ErrorEnvelope | ExportJob | None:
    """Start an async export job — the signed evidence bundle, or the digest-only chain stream for client-
    side verification.

     Requires: admin key role. The export act is itself journalled; simulations are excluded unless asked
    for and then listed separately.

    Args:
        idempotency_key (str):
        body (ExportRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ExportJob
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
