from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.start_page_act_body import StartPageActBody
from ...types import Response


def _get_kwargs(
    *,
    body: StartPageActBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/web/start",
    }

    _kwargs["data"] = body.to_dict()
    headers["Content-Type"] = "application/x-www-form-urlencoded"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | str | None:
    if response.status_code == 200:
        response_200 = response.text
        return response_200

    if response.status_code == 303:
        response_303 = cast(Any, None)
        return response_303

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any | str]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: StartPageActBody,
) -> Response[Any | str]:
    """One act of the onboarding page, named by action.

     Requires a session, csrf_token and the same-origin check. issue_claude_ai issues a connector key
    <user>-claude-ai and shows its connect code once; issue_claude_code and issue_other issue
    <user>-claude-code and <user>-agent and show the value once; a taken name gains -2 to -9. Past the
    keys quota, limit.quota. request_deletion with confirm=delete journals web.deletion_requested
    (ADR-0068 d6, d10). Proposed.

    Args:
        body (StartPageActBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: StartPageActBody,
) -> Any | str | None:
    """One act of the onboarding page, named by action.

     Requires a session, csrf_token and the same-origin check. issue_claude_ai issues a connector key
    <user>-claude-ai and shows its connect code once; issue_claude_code and issue_other issue
    <user>-claude-code and <user>-agent and show the value once; a taken name gains -2 to -9. Past the
    keys quota, limit.quota. request_deletion with confirm=delete journals web.deletion_requested
    (ADR-0068 d6, d10). Proposed.

    Args:
        body (StartPageActBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: StartPageActBody,
) -> Response[Any | str]:
    """One act of the onboarding page, named by action.

     Requires a session, csrf_token and the same-origin check. issue_claude_ai issues a connector key
    <user>-claude-ai and shows its connect code once; issue_claude_code and issue_other issue
    <user>-claude-code and <user>-agent and show the value once; a taken name gains -2 to -9. Past the
    keys quota, limit.quota. request_deletion with confirm=delete journals web.deletion_requested
    (ADR-0068 d6, d10). Proposed.

    Args:
        body (StartPageActBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: StartPageActBody,
) -> Any | str | None:
    """One act of the onboarding page, named by action.

     Requires a session, csrf_token and the same-origin check. issue_claude_ai issues a connector key
    <user>-claude-ai and shows its connect code once; issue_claude_code and issue_other issue
    <user>-claude-code and <user>-agent and show the value once; a taken name gains -2 to -9. Past the
    keys quota, limit.quota. request_deletion with confirm=delete journals web.deletion_requested
    (ADR-0068 d6, d10). Proposed.

    Args:
        body (StartPageActBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
