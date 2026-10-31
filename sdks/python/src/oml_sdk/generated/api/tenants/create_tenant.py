from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.create_tenant_body import CreateTenantBody
from ...models.create_tenant_response_201 import CreateTenantResponse201
from ...models.error_envelope import ErrorEnvelope
from ...types import Response


def _get_kwargs(
    *,
    body: CreateTenantBody,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/tenants",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> CreateTenantResponse201 | ErrorEnvelope:
    if response.status_code == 201:
        response_201 = CreateTenantResponse201.from_dict(response.json())

        return response_201

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
) -> Response[CreateTenantResponse201 | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: CreateTenantBody,
    idempotency_key: str,
) -> Response[CreateTenantResponse201 | ErrorEnvelope]:
    """Create a tenant and its universe root; the operator then issues its first admin key and nothing
    else.

     Requires: operator key role. name must be unique in the deployment (409 name.taken — ADR-0026 d1,
    d5); the tenant's root workarea is created with the name universe (ADR-0026 d6). Operator keys never
    read, write or review content (access.verb_ceiling).

    Args:
        idempotency_key (str):
        body (CreateTenantBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[CreateTenantResponse201 | ErrorEnvelope]
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
    body: CreateTenantBody,
    idempotency_key: str,
) -> CreateTenantResponse201 | ErrorEnvelope | None:
    """Create a tenant and its universe root; the operator then issues its first admin key and nothing
    else.

     Requires: operator key role. name must be unique in the deployment (409 name.taken — ADR-0026 d1,
    d5); the tenant's root workarea is created with the name universe (ADR-0026 d6). Operator keys never
    read, write or review content (access.verb_ceiling).

    Args:
        idempotency_key (str):
        body (CreateTenantBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        CreateTenantResponse201 | ErrorEnvelope
    """

    return sync_detailed(
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: CreateTenantBody,
    idempotency_key: str,
) -> Response[CreateTenantResponse201 | ErrorEnvelope]:
    """Create a tenant and its universe root; the operator then issues its first admin key and nothing
    else.

     Requires: operator key role. name must be unique in the deployment (409 name.taken — ADR-0026 d1,
    d5); the tenant's root workarea is created with the name universe (ADR-0026 d6). Operator keys never
    read, write or review content (access.verb_ceiling).

    Args:
        idempotency_key (str):
        body (CreateTenantBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[CreateTenantResponse201 | ErrorEnvelope]
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
    body: CreateTenantBody,
    idempotency_key: str,
) -> CreateTenantResponse201 | ErrorEnvelope | None:
    """Create a tenant and its universe root; the operator then issues its first admin key and nothing
    else.

     Requires: operator key role. name must be unique in the deployment (409 name.taken — ADR-0026 d1,
    d5); the tenant's root workarea is created with the name universe (ADR-0026 d6). Operator keys never
    read, write or review content (access.verb_ceiling).

    Args:
        idempotency_key (str):
        body (CreateTenantBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        CreateTenantResponse201 | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
