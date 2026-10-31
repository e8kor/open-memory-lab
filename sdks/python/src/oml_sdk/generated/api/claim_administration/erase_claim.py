from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.act_result import ActResult
from ...models.erase_request import EraseRequest
from ...models.error_envelope import ErrorEnvelope
from ...types import Response


def _get_kwargs(
    artifact_id: str,
    *,
    body: EraseRequest,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/memories/{artifact_id}/erase".format(
            artifact_id=quote(str(artifact_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ActResult | ErrorEnvelope:
    if response.status_code == 202:
        response_202 = ActResult.from_dict(response.json())

        return response_202

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ActResult | ErrorEnvelope]:
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
    body: EraseRequest,
    idempotency_key: str,
) -> Response[ActResult | ErrorEnvelope]:
    """Journalled crypto-shred — erase event plus DEK destruction, scoped to body and attachment bytes.

     Requires: admin key role. dry_run returns the erasure inventory per copy class. Attribution metadata
    survives — FR-1.

    Args:
        artifact_id (str):
        idempotency_key (str):
        body (EraseRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ActResult | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
        body=body,
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
    body: EraseRequest,
    idempotency_key: str,
) -> ActResult | ErrorEnvelope | None:
    """Journalled crypto-shred — erase event plus DEK destruction, scoped to body and attachment bytes.

     Requires: admin key role. dry_run returns the erasure inventory per copy class. Attribution metadata
    survives — FR-1.

    Args:
        artifact_id (str):
        idempotency_key (str):
        body (EraseRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ActResult | ErrorEnvelope
    """

    return sync_detailed(
        artifact_id=artifact_id,
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: EraseRequest,
    idempotency_key: str,
) -> Response[ActResult | ErrorEnvelope]:
    """Journalled crypto-shred — erase event plus DEK destruction, scoped to body and attachment bytes.

     Requires: admin key role. dry_run returns the erasure inventory per copy class. Attribution metadata
    survives — FR-1.

    Args:
        artifact_id (str):
        idempotency_key (str):
        body (EraseRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ActResult | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
        body=body,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: EraseRequest,
    idempotency_key: str,
) -> ActResult | ErrorEnvelope | None:
    """Journalled crypto-shred — erase event plus DEK destruction, scoped to body and attachment bytes.

     Requires: admin key role. dry_run returns the erasure inventory per copy class. Attribution metadata
    survives — FR-1.

    Args:
        artifact_id (str):
        idempotency_key (str):
        body (EraseRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ActResult | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            artifact_id=artifact_id,
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
