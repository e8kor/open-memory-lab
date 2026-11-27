from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.workspace import Workspace
from ...models.workspace_create import WorkspaceCreate
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: WorkspaceCreate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/workspaces",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | Workspace:
    if response.status_code == 201:
        response_201 = Workspace.from_dict(response.json())

        return response_201

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

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
) -> Response[ErrorEnvelope | Workspace]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: WorkspaceCreate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | Workspace]:
    """Create a child workspace in development.

     Requires: admin key role. parent and crew each take an id or a name, and parent_id, the v0.3.0
    spelling, is refused with request.validation, details.renamed_to /parent (ADR-0039 d7); a crew that
    names no crew record of the tenant answers access.not_found, checked after the parent and before
    name.taken. name must be unique in the tenant (409 name.taken, checked after every other check;
    universe is always taken, by the root). Refused under a frozen parent (workspace.frozen); universe
    cannot be created or deleted. A resubmission under the same Idempotency-Key and body answers the
    recorded creation with Idempotency-Replay: true and creates nothing again; the same key with another
    body is idempotency.digest_mismatch (ADR-0070 d3 layer 4; since v0.7.0). A person's web session key
    (<user>-web, through its capacity credential) creates under its own root, as the web console's
    Workspaces view does (ADR-0070 d5); a person's admin session key (<user>-admin) creates anywhere in
    the tenant, under universe included, as the admin Workspaces page does, a neutral admin act
    (ADR-0097 d9, d29; since v0.8.6). Any key whose effective access at the parent comes from a grant
    carrying create creates there, a shared parent included; under a node grant the child gets the same
    grant in the create's commit, and a frozen parent refuses it with access.verb_ceiling,
    details.reason parent_frozen (ADR-0097 d31; since v0.8.6). A node below a user root counts on that
    account's workspaces meter, whoever creates it; past the account's limit the create is 429
    limit.quota with details.quota workspaces, and nothing is journalled (ADR-0095 d4; since v0.8.5).
    MCP's oml_write create_workspace rides this operation for a contributor key that names a user, and
    for a key a grant carrying create admits at the parent; every other key is access.verb_ceiling with
    details.reason create_is_own_root_only there (ADR-0095 d3; ADR-0097 d31).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (WorkspaceCreate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Workspace]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: WorkspaceCreate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | Workspace | None:
    """Create a child workspace in development.

     Requires: admin key role. parent and crew each take an id or a name, and parent_id, the v0.3.0
    spelling, is refused with request.validation, details.renamed_to /parent (ADR-0039 d7); a crew that
    names no crew record of the tenant answers access.not_found, checked after the parent and before
    name.taken. name must be unique in the tenant (409 name.taken, checked after every other check;
    universe is always taken, by the root). Refused under a frozen parent (workspace.frozen); universe
    cannot be created or deleted. A resubmission under the same Idempotency-Key and body answers the
    recorded creation with Idempotency-Replay: true and creates nothing again; the same key with another
    body is idempotency.digest_mismatch (ADR-0070 d3 layer 4; since v0.7.0). A person's web session key
    (<user>-web, through its capacity credential) creates under its own root, as the web console's
    Workspaces view does (ADR-0070 d5); a person's admin session key (<user>-admin) creates anywhere in
    the tenant, under universe included, as the admin Workspaces page does, a neutral admin act
    (ADR-0097 d9, d29; since v0.8.6). Any key whose effective access at the parent comes from a grant
    carrying create creates there, a shared parent included; under a node grant the child gets the same
    grant in the create's commit, and a frozen parent refuses it with access.verb_ceiling,
    details.reason parent_frozen (ADR-0097 d31; since v0.8.6). A node below a user root counts on that
    account's workspaces meter, whoever creates it; past the account's limit the create is 429
    limit.quota with details.quota workspaces, and nothing is journalled (ADR-0095 d4; since v0.8.5).
    MCP's oml_write create_workspace rides this operation for a contributor key that names a user, and
    for a key a grant carrying create admits at the parent; every other key is access.verb_ceiling with
    details.reason create_is_own_root_only there (ADR-0095 d3; ADR-0097 d31).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (WorkspaceCreate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Workspace
    """

    return sync_detailed(
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: WorkspaceCreate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | Workspace]:
    """Create a child workspace in development.

     Requires: admin key role. parent and crew each take an id or a name, and parent_id, the v0.3.0
    spelling, is refused with request.validation, details.renamed_to /parent (ADR-0039 d7); a crew that
    names no crew record of the tenant answers access.not_found, checked after the parent and before
    name.taken. name must be unique in the tenant (409 name.taken, checked after every other check;
    universe is always taken, by the root). Refused under a frozen parent (workspace.frozen); universe
    cannot be created or deleted. A resubmission under the same Idempotency-Key and body answers the
    recorded creation with Idempotency-Replay: true and creates nothing again; the same key with another
    body is idempotency.digest_mismatch (ADR-0070 d3 layer 4; since v0.7.0). A person's web session key
    (<user>-web, through its capacity credential) creates under its own root, as the web console's
    Workspaces view does (ADR-0070 d5); a person's admin session key (<user>-admin) creates anywhere in
    the tenant, under universe included, as the admin Workspaces page does, a neutral admin act
    (ADR-0097 d9, d29; since v0.8.6). Any key whose effective access at the parent comes from a grant
    carrying create creates there, a shared parent included; under a node grant the child gets the same
    grant in the create's commit, and a frozen parent refuses it with access.verb_ceiling,
    details.reason parent_frozen (ADR-0097 d31; since v0.8.6). A node below a user root counts on that
    account's workspaces meter, whoever creates it; past the account's limit the create is 429
    limit.quota with details.quota workspaces, and nothing is journalled (ADR-0095 d4; since v0.8.5).
    MCP's oml_write create_workspace rides this operation for a contributor key that names a user, and
    for a key a grant carrying create admits at the parent; every other key is access.verb_ceiling with
    details.reason create_is_own_root_only there (ADR-0095 d3; ADR-0097 d31).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (WorkspaceCreate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Workspace]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: WorkspaceCreate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | Workspace | None:
    """Create a child workspace in development.

     Requires: admin key role. parent and crew each take an id or a name, and parent_id, the v0.3.0
    spelling, is refused with request.validation, details.renamed_to /parent (ADR-0039 d7); a crew that
    names no crew record of the tenant answers access.not_found, checked after the parent and before
    name.taken. name must be unique in the tenant (409 name.taken, checked after every other check;
    universe is always taken, by the root). Refused under a frozen parent (workspace.frozen); universe
    cannot be created or deleted. A resubmission under the same Idempotency-Key and body answers the
    recorded creation with Idempotency-Replay: true and creates nothing again; the same key with another
    body is idempotency.digest_mismatch (ADR-0070 d3 layer 4; since v0.7.0). A person's web session key
    (<user>-web, through its capacity credential) creates under its own root, as the web console's
    Workspaces view does (ADR-0070 d5); a person's admin session key (<user>-admin) creates anywhere in
    the tenant, under universe included, as the admin Workspaces page does, a neutral admin act
    (ADR-0097 d9, d29; since v0.8.6). Any key whose effective access at the parent comes from a grant
    carrying create creates there, a shared parent included; under a node grant the child gets the same
    grant in the create's commit, and a frozen parent refuses it with access.verb_ceiling,
    details.reason parent_frozen (ADR-0097 d31; since v0.8.6). A node below a user root counts on that
    account's workspaces meter, whoever creates it; past the account's limit the create is 429
    limit.quota with details.quota workspaces, and nothing is journalled (ADR-0095 d4; since v0.8.5).
    MCP's oml_write create_workspace rides this operation for a contributor key that names a user, and
    for a key a grant carrying create admits at the parent; every other key is access.verb_ceiling with
    details.reason create_is_own_root_only there (ADR-0095 d3; ADR-0097 d31).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (WorkspaceCreate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Workspace
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            idempotency_key=idempotency_key,
        )
    ).parsed
