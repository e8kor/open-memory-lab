from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.act_result import ActResult
from ...models.error_envelope import ErrorEnvelope
from ...models.reparent_request import ReparentRequest
from ...types import Response


def _get_kwargs(
    workarea_id: str,
    *,
    body: ReparentRequest,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/workareas/{workarea_id}/reparent".format(
            workarea_id=quote(str(workarea_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ActResult | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = ActResult.from_dict(response.json())

        return response_200

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

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
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: ReparentRequest,
    idempotency_key: str,
) -> Response[ActResult | ErrorEnvelope]:
    """Journalled single-transaction descendant path rewrite; dry_run previews paths and grants affected.

     Requires: admin key role. Re-parenting into a validated destination is refused (workarea.frozen).

    Args:
        workarea_id (str):
        idempotency_key (str):
        body (ReparentRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ActResult | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        workarea_id=workarea_id,
        body=body,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: ReparentRequest,
    idempotency_key: str,
) -> ActResult | ErrorEnvelope | None:
    """Journalled single-transaction descendant path rewrite; dry_run previews paths and grants affected.

     Requires: admin key role. Re-parenting into a validated destination is refused (workarea.frozen).

    Args:
        workarea_id (str):
        idempotency_key (str):
        body (ReparentRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ActResult | ErrorEnvelope
    """

    return sync_detailed(
        workarea_id=workarea_id,
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: ReparentRequest,
    idempotency_key: str,
) -> Response[ActResult | ErrorEnvelope]:
    """Journalled single-transaction descendant path rewrite; dry_run previews paths and grants affected.

     Requires: admin key role. Re-parenting into a validated destination is refused (workarea.frozen).

    Args:
        workarea_id (str):
        idempotency_key (str):
        body (ReparentRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ActResult | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        workarea_id=workarea_id,
        body=body,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: ReparentRequest,
    idempotency_key: str,
) -> ActResult | ErrorEnvelope | None:
    """Journalled single-transaction descendant path rewrite; dry_run previews paths and grants affected.

     Requires: admin key role. Re-parenting into a validated destination is refused (workarea.frozen).

    Args:
        workarea_id (str):
        idempotency_key (str):
        body (ReparentRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ActResult | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            workarea_id=workarea_id,
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
