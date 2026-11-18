from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.workspace_view import WorkspaceView
from ...types import UNSET, Response, Unset


def _get_kwargs(
    workspace: str,
    *,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/workspaces/{workspace}".format(
            workspace=quote(str(workspace), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | WorkspaceView:
    if response.status_code == 200:
        response_200 = WorkspaceView.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | WorkspaceView]:
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
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | WorkspaceView]:
    """One node — id, parent, derived path, status, crew record — with the calling key's view of it.

     Requires: reader effective access (access.not_found otherwise), or the admin key role, which reads
    the structure of every workspace in its tenant without grants and never claim content — for an admin
    key every child and edge is present and my_effective_verbs is empty (docs/architecture/roles-and-
    keys.md, permission matrix footnote 6; ADR-0026 d15). children, my_effective_verbs and
    permission_epoch are relative to the calling key: children (ordered by path_leaf, then id) and edges
    (ordered by edge_id) outside read access are absent; my_effective_verbs is the union over the grant
    roles effective on the node (the effective-verbs table in docs/architecture/roles-and-keys.md), with
    remember for a person's session reviewer key where it holds reviewer and reader (ADR-0082 d11);
    permission_epoch is the calling key's permission epoch as declared in
    docs/architecture/workspaces.md (ADR-0025 d14). One call carries all of it.

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

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WorkspaceView]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | WorkspaceView | None:
    """One node — id, parent, derived path, status, crew record — with the calling key's view of it.

     Requires: reader effective access (access.not_found otherwise), or the admin key role, which reads
    the structure of every workspace in its tenant without grants and never claim content — for an admin
    key every child and edge is present and my_effective_verbs is empty (docs/architecture/roles-and-
    keys.md, permission matrix footnote 6; ADR-0026 d15). children, my_effective_verbs and
    permission_epoch are relative to the calling key: children (ordered by path_leaf, then id) and edges
    (ordered by edge_id) outside read access are absent; my_effective_verbs is the union over the grant
    roles effective on the node (the effective-verbs table in docs/architecture/roles-and-keys.md), with
    remember for a person's session reviewer key where it holds reviewer and reader (ADR-0082 d11);
    permission_epoch is the calling key's permission epoch as declared in
    docs/architecture/workspaces.md (ADR-0025 d14). One call carries all of it.

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

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WorkspaceView
    """

    return sync_detailed(
        workspace=workspace,
        client=client,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | WorkspaceView]:
    """One node — id, parent, derived path, status, crew record — with the calling key's view of it.

     Requires: reader effective access (access.not_found otherwise), or the admin key role, which reads
    the structure of every workspace in its tenant without grants and never claim content — for an admin
    key every child and edge is present and my_effective_verbs is empty (docs/architecture/roles-and-
    keys.md, permission matrix footnote 6; ADR-0026 d15). children, my_effective_verbs and
    permission_epoch are relative to the calling key: children (ordered by path_leaf, then id) and edges
    (ordered by edge_id) outside read access are absent; my_effective_verbs is the union over the grant
    roles effective on the node (the effective-verbs table in docs/architecture/roles-and-keys.md), with
    remember for a person's session reviewer key where it holds reviewer and reader (ADR-0082 d11);
    permission_epoch is the calling key's permission epoch as declared in
    docs/architecture/workspaces.md (ADR-0025 d14). One call carries all of it.

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

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WorkspaceView]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | WorkspaceView | None:
    """One node — id, parent, derived path, status, crew record — with the calling key's view of it.

     Requires: reader effective access (access.not_found otherwise), or the admin key role, which reads
    the structure of every workspace in its tenant without grants and never claim content — for an admin
    key every child and edge is present and my_effective_verbs is empty (docs/architecture/roles-and-
    keys.md, permission matrix footnote 6; ADR-0026 d15). children, my_effective_verbs and
    permission_epoch are relative to the calling key: children (ordered by path_leaf, then id) and edges
    (ordered by edge_id) outside read access are absent; my_effective_verbs is the union over the grant
    roles effective on the node (the effective-verbs table in docs/architecture/roles-and-keys.md), with
    remember for a person's session reviewer key where it holds reviewer and reader (ADR-0082 d11);
    permission_epoch is the calling key's permission epoch as declared in
    docs/architecture/workspaces.md (ADR-0025 d14). One call carries all of it.

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

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WorkspaceView
    """

    return (
        await asyncio_detailed(
            workspace=workspace,
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
