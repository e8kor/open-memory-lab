from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.set_workspace_federation_body import SetWorkspaceFederationBody
from ...models.set_workspace_federation_response_200 import (
    SetWorkspaceFederationResponse200,
)
from ...types import UNSET, Response, Unset


def _get_kwargs(
    workspace: str,
    *,
    body: SetWorkspaceFederationBody,
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
        "url": "/v2/workspaces/{workspace}/federation".format(
            workspace=quote(str(workspace), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | SetWorkspaceFederationResponse200:
    if response.status_code == 200:
        response_200 = SetWorkspaceFederationResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | SetWorkspaceFederationResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    body: SetWorkspaceFederationBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | SetWorkspaceFederationResponse200]:
    """Whether recalls scoped elsewhere may search this workspace when they ask for wider sources
    (ADR-0072).

     Requires: a reviewer key used_by person with reviewer effective access on the node, or an admin key;
    every other key is refused before any lookup with access.verb_ceiling, details.reason
    federation_is_human_only (ADR-0072 d5). It grants nothing: off, a key that reads the node still
    recalls there by naming it as the scope. Setting the value the node holds journals nothing;
    otherwise journals workspace.federation_set. Since v0.7.0.

    Args:
        workspace (str): A workspace, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SetWorkspaceFederationBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SetWorkspaceFederationResponse200]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
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
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    body: SetWorkspaceFederationBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | SetWorkspaceFederationResponse200 | None:
    """Whether recalls scoped elsewhere may search this workspace when they ask for wider sources
    (ADR-0072).

     Requires: a reviewer key used_by person with reviewer effective access on the node, or an admin key;
    every other key is refused before any lookup with access.verb_ceiling, details.reason
    federation_is_human_only (ADR-0072 d5). It grants nothing: off, a key that reads the node still
    recalls there by naming it as the scope. Setting the value the node holds journals nothing;
    otherwise journals workspace.federation_set. Since v0.7.0.

    Args:
        workspace (str): A workspace, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SetWorkspaceFederationBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SetWorkspaceFederationResponse200
    """

    return sync_detailed(
        workspace=workspace,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    body: SetWorkspaceFederationBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | SetWorkspaceFederationResponse200]:
    """Whether recalls scoped elsewhere may search this workspace when they ask for wider sources
    (ADR-0072).

     Requires: a reviewer key used_by person with reviewer effective access on the node, or an admin key;
    every other key is refused before any lookup with access.verb_ceiling, details.reason
    federation_is_human_only (ADR-0072 d5). It grants nothing: off, a key that reads the node still
    recalls there by naming it as the scope. Setting the value the node holds journals nothing;
    otherwise journals workspace.federation_set. Since v0.7.0.

    Args:
        workspace (str): A workspace, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SetWorkspaceFederationBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SetWorkspaceFederationResponse200]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    body: SetWorkspaceFederationBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | SetWorkspaceFederationResponse200 | None:
    """Whether recalls scoped elsewhere may search this workspace when they ask for wider sources
    (ADR-0072).

     Requires: a reviewer key used_by person with reviewer effective access on the node, or an admin key;
    every other key is refused before any lookup with access.verb_ceiling, details.reason
    federation_is_human_only (ADR-0072 d5). It grants nothing: off, a key that reads the node still
    recalls there by naming it as the scope. Setting the value the node holds journals nothing;
    otherwise journals workspace.federation_set. Since v0.7.0.

    Args:
        workspace (str): A workspace, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SetWorkspaceFederationBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SetWorkspaceFederationResponse200
    """

    return (
        await asyncio_detailed(
            workspace=workspace,
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            idempotency_key=idempotency_key,
        )
    ).parsed
