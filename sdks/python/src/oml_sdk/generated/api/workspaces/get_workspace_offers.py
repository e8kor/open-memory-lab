from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.workspace_offers import WorkspaceOffers
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
        "url": "/v2/workspaces/{workspace}/offers".format(
            workspace=quote(str(workspace), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | WorkspaceOffers:
    if response.status_code == 200:
        response_200 = WorkspaceOffers.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | WorkspaceOffers]:
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
) -> Response[ErrorEnvelope | WorkspaceOffers]:
    """The transitions, the mode change and the commit the caller's verbs admit on a workspace now.

     Requires: reader effective access on the workspace, or the admin key role; one the caller cannot
    read is access.not_found (R3). Each act is offered only where the verb's own check admits the caller
    now: propose the next rung, ratify a live proposal another key made, put the workspace in progress,
    or commit an in-progress root; a commit the held rows refuse is not offered and names their count.
    refused names each of proposing protected, proposing frozen, mode in_progress and mode standard that
    is not offered, with the code and message its verb would refuse it with now, so a confirmation names
    the refusal without acting. Journals nothing (ADR-0045 d28; ADR-0070 d5). Since v0.6.4.

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
        Response[ErrorEnvelope | WorkspaceOffers]
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
) -> ErrorEnvelope | WorkspaceOffers | None:
    """The transitions, the mode change and the commit the caller's verbs admit on a workspace now.

     Requires: reader effective access on the workspace, or the admin key role; one the caller cannot
    read is access.not_found (R3). Each act is offered only where the verb's own check admits the caller
    now: propose the next rung, ratify a live proposal another key made, put the workspace in progress,
    or commit an in-progress root; a commit the held rows refuse is not offered and names their count.
    refused names each of proposing protected, proposing frozen, mode in_progress and mode standard that
    is not offered, with the code and message its verb would refuse it with now, so a confirmation names
    the refusal without acting. Journals nothing (ADR-0045 d28; ADR-0070 d5). Since v0.6.4.

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
        ErrorEnvelope | WorkspaceOffers
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
) -> Response[ErrorEnvelope | WorkspaceOffers]:
    """The transitions, the mode change and the commit the caller's verbs admit on a workspace now.

     Requires: reader effective access on the workspace, or the admin key role; one the caller cannot
    read is access.not_found (R3). Each act is offered only where the verb's own check admits the caller
    now: propose the next rung, ratify a live proposal another key made, put the workspace in progress,
    or commit an in-progress root; a commit the held rows refuse is not offered and names their count.
    refused names each of proposing protected, proposing frozen, mode in_progress and mode standard that
    is not offered, with the code and message its verb would refuse it with now, so a confirmation names
    the refusal without acting. Journals nothing (ADR-0045 d28; ADR-0070 d5). Since v0.6.4.

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
        Response[ErrorEnvelope | WorkspaceOffers]
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
) -> ErrorEnvelope | WorkspaceOffers | None:
    """The transitions, the mode change and the commit the caller's verbs admit on a workspace now.

     Requires: reader effective access on the workspace, or the admin key role; one the caller cannot
    read is access.not_found (R3). Each act is offered only where the verb's own check admits the caller
    now: propose the next rung, ratify a live proposal another key made, put the workspace in progress,
    or commit an in-progress root; a commit the held rows refuse is not offered and names their count.
    refused names each of proposing protected, proposing frozen, mode in_progress and mode standard that
    is not offered, with the code and message its verb would refuse it with now, so a confirmation names
    the refusal without acting. Journals nothing (ADR-0045 d28; ADR-0070 d5). Since v0.6.4.

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
        ErrorEnvelope | WorkspaceOffers
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
