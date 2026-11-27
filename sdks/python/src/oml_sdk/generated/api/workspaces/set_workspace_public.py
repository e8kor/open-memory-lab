from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.set_workspace_public_body import SetWorkspacePublicBody
from ...models.set_workspace_public_response_200 import SetWorkspacePublicResponse200
from ...models.set_workspace_public_response_202 import SetWorkspacePublicResponse202
from ...types import UNSET, Response, Unset


def _get_kwargs(
    workspace: str,
    *,
    body: SetWorkspacePublicBody,
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
        "url": "/v2/workspaces/{workspace}/public".format(
            workspace=quote(str(workspace), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | SetWorkspacePublicResponse200 | SetWorkspacePublicResponse202:
    if response.status_code == 200:
        response_200 = SetWorkspacePublicResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 202:
        response_202 = SetWorkspacePublicResponse202.from_dict(response.json())

        return response_202

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[
    ErrorEnvelope | SetWorkspacePublicResponse200 | SetWorkspacePublicResponse202
]:
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
    body: SetWorkspacePublicBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[
    ErrorEnvelope | SetWorkspacePublicResponse200 | SetWorkspacePublicResponse202
]:
    """An admin person marks a protected or frozen workspace public, so every reading key of the tenant
    reads its approved items and recalls them by default, or clears the mark (ADR-0098).

     Requires: an admin person, a linked admin key or <user>-admin; every other key, a reviewer key on
    the workspace included, is refused before any lookup with access.verb_ceiling, details.reason
    public_is_admin_only (ADR-0098 d6). A set is an admin act of class widening: it takes effect only
    when a second admin person confirms it with confirmAdminAct, and at once under sole_admin, marked; a
    reason is required. A clear is narrowing and takes effect at once, declining a set that waits.
    Refused workspace.public_refused with details.reason status (a development workspace), user_root (a
    user root or a workspace under one), pending (a set already waits; details.act_id names it),
    already_public, or not_public. The effect journals workspace.public_set in the commit where the act
    takes effect. The flag covers the node alone, never its children. Since v0.8.6.

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
        body (SetWorkspacePublicBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SetWorkspacePublicResponse200 | SetWorkspacePublicResponse202]
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
    body: SetWorkspacePublicBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> (
    ErrorEnvelope | SetWorkspacePublicResponse200 | SetWorkspacePublicResponse202 | None
):
    """An admin person marks a protected or frozen workspace public, so every reading key of the tenant
    reads its approved items and recalls them by default, or clears the mark (ADR-0098).

     Requires: an admin person, a linked admin key or <user>-admin; every other key, a reviewer key on
    the workspace included, is refused before any lookup with access.verb_ceiling, details.reason
    public_is_admin_only (ADR-0098 d6). A set is an admin act of class widening: it takes effect only
    when a second admin person confirms it with confirmAdminAct, and at once under sole_admin, marked; a
    reason is required. A clear is narrowing and takes effect at once, declining a set that waits.
    Refused workspace.public_refused with details.reason status (a development workspace), user_root (a
    user root or a workspace under one), pending (a set already waits; details.act_id names it),
    already_public, or not_public. The effect journals workspace.public_set in the commit where the act
    takes effect. The flag covers the node alone, never its children. Since v0.8.6.

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
        body (SetWorkspacePublicBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SetWorkspacePublicResponse200 | SetWorkspacePublicResponse202
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
    body: SetWorkspacePublicBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[
    ErrorEnvelope | SetWorkspacePublicResponse200 | SetWorkspacePublicResponse202
]:
    """An admin person marks a protected or frozen workspace public, so every reading key of the tenant
    reads its approved items and recalls them by default, or clears the mark (ADR-0098).

     Requires: an admin person, a linked admin key or <user>-admin; every other key, a reviewer key on
    the workspace included, is refused before any lookup with access.verb_ceiling, details.reason
    public_is_admin_only (ADR-0098 d6). A set is an admin act of class widening: it takes effect only
    when a second admin person confirms it with confirmAdminAct, and at once under sole_admin, marked; a
    reason is required. A clear is narrowing and takes effect at once, declining a set that waits.
    Refused workspace.public_refused with details.reason status (a development workspace), user_root (a
    user root or a workspace under one), pending (a set already waits; details.act_id names it),
    already_public, or not_public. The effect journals workspace.public_set in the commit where the act
    takes effect. The flag covers the node alone, never its children. Since v0.8.6.

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
        body (SetWorkspacePublicBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SetWorkspacePublicResponse200 | SetWorkspacePublicResponse202]
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
    body: SetWorkspacePublicBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> (
    ErrorEnvelope | SetWorkspacePublicResponse200 | SetWorkspacePublicResponse202 | None
):
    """An admin person marks a protected or frozen workspace public, so every reading key of the tenant
    reads its approved items and recalls them by default, or clears the mark (ADR-0098).

     Requires: an admin person, a linked admin key or <user>-admin; every other key, a reviewer key on
    the workspace included, is refused before any lookup with access.verb_ceiling, details.reason
    public_is_admin_only (ADR-0098 d6). A set is an admin act of class widening: it takes effect only
    when a second admin person confirms it with confirmAdminAct, and at once under sole_admin, marked; a
    reason is required. A clear is narrowing and takes effect at once, declining a set that waits.
    Refused workspace.public_refused with details.reason status (a development workspace), user_root (a
    user root or a workspace under one), pending (a set already waits; details.act_id names it),
    already_public, or not_public. The effect journals workspace.public_set in the commit where the act
    takes effect. The flag covers the node alone, never its children. Since v0.8.6.

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
        body (SetWorkspacePublicBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SetWorkspacePublicResponse200 | SetWorkspacePublicResponse202
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
