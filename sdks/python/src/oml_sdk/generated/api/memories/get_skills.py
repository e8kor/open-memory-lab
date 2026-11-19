from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.skill_set import SkillSet
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workspace: str | Unset = UNSET,
    include_own_held: bool | Unset = False,
    cursor: str | Unset = UNSET,
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

    params: dict[str, Any] = {}

    params["workspace"] = workspace

    params["include_own_held"] = include_own_held

    params["cursor"] = cursor

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/skills",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | SkillSet:
    if response.status_code == 200:
        response_200 = SkillSet.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = ErrorEnvelope.from_dict(response.json())

        return response_401

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | SkillSet]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    include_own_held: bool | Unset = False,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | SkillSet]:
    """The skills offered at the scope node, 50 a page, without bodies.

     Requires: reader on the scope node. Offers the promoted skill tips on the node and, under
    guardrail_scope ancestors, on each ancestor the key reads and, inside a workspace a person put in
    progress, on each node below the scope node in that subtree; never a skill in a child workspace
    outside one. Inside an in-progress subtree every author's held skill is offered too, and under
    include_own_held the key's own: marked unreviewed, never a held retirement or a version the scan
    held. A name held in two workspaces goes to the one nearer the scope node by tree distance, an
    ancestor winning a tie and then the smaller path; the nearer names the farther in overrides. Ordered
    by depth, then name; any parameter other than workspace, include_own_held and cursor is
    request.validation naming it. Serving the set is a read, not a recall (docs/api/http-api.md, Skills;
    ADR-0080 d4, d5, d10, d11, d12, d14, d16; since v0.8.3).

    Args:
        workspace (str | Unset):
        include_own_held (bool | Unset):  Default: False.
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SkillSet]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        include_own_held=include_own_held,
        cursor=cursor,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    include_own_held: bool | Unset = False,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | SkillSet | None:
    """The skills offered at the scope node, 50 a page, without bodies.

     Requires: reader on the scope node. Offers the promoted skill tips on the node and, under
    guardrail_scope ancestors, on each ancestor the key reads and, inside a workspace a person put in
    progress, on each node below the scope node in that subtree; never a skill in a child workspace
    outside one. Inside an in-progress subtree every author's held skill is offered too, and under
    include_own_held the key's own: marked unreviewed, never a held retirement or a version the scan
    held. A name held in two workspaces goes to the one nearer the scope node by tree distance, an
    ancestor winning a tie and then the smaller path; the nearer names the farther in overrides. Ordered
    by depth, then name; any parameter other than workspace, include_own_held and cursor is
    request.validation naming it. Serving the set is a read, not a recall (docs/api/http-api.md, Skills;
    ADR-0080 d4, d5, d10, d11, d12, d14, d16; since v0.8.3).

    Args:
        workspace (str | Unset):
        include_own_held (bool | Unset):  Default: False.
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SkillSet
    """

    return sync_detailed(
        client=client,
        workspace=workspace,
        include_own_held=include_own_held,
        cursor=cursor,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    include_own_held: bool | Unset = False,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | SkillSet]:
    """The skills offered at the scope node, 50 a page, without bodies.

     Requires: reader on the scope node. Offers the promoted skill tips on the node and, under
    guardrail_scope ancestors, on each ancestor the key reads and, inside a workspace a person put in
    progress, on each node below the scope node in that subtree; never a skill in a child workspace
    outside one. Inside an in-progress subtree every author's held skill is offered too, and under
    include_own_held the key's own: marked unreviewed, never a held retirement or a version the scan
    held. A name held in two workspaces goes to the one nearer the scope node by tree distance, an
    ancestor winning a tie and then the smaller path; the nearer names the farther in overrides. Ordered
    by depth, then name; any parameter other than workspace, include_own_held and cursor is
    request.validation naming it. Serving the set is a read, not a recall (docs/api/http-api.md, Skills;
    ADR-0080 d4, d5, d10, d11, d12, d14, d16; since v0.8.3).

    Args:
        workspace (str | Unset):
        include_own_held (bool | Unset):  Default: False.
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SkillSet]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        include_own_held=include_own_held,
        cursor=cursor,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    include_own_held: bool | Unset = False,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | SkillSet | None:
    """The skills offered at the scope node, 50 a page, without bodies.

     Requires: reader on the scope node. Offers the promoted skill tips on the node and, under
    guardrail_scope ancestors, on each ancestor the key reads and, inside a workspace a person put in
    progress, on each node below the scope node in that subtree; never a skill in a child workspace
    outside one. Inside an in-progress subtree every author's held skill is offered too, and under
    include_own_held the key's own: marked unreviewed, never a held retirement or a version the scan
    held. A name held in two workspaces goes to the one nearer the scope node by tree distance, an
    ancestor winning a tie and then the smaller path; the nearer names the farther in overrides. Ordered
    by depth, then name; any parameter other than workspace, include_own_held and cursor is
    request.validation naming it. Serving the set is a read, not a recall (docs/api/http-api.md, Skills;
    ADR-0080 d4, d5, d10, d11, d12, d14, d16; since v0.8.3).

    Args:
        workspace (str | Unset):
        include_own_held (bool | Unset):  Default: False.
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SkillSet
    """

    return (
        await asyncio_detailed(
            client=client,
            workspace=workspace,
            include_own_held=include_own_held,
            cursor=cursor,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
