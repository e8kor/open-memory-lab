from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.users_page_act_body import UsersPageActBody
from ...types import Response


def _get_kwargs(
    *,
    body: UsersPageActBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/web/admin/users",
    }

    _kwargs["data"] = body.to_dict()
    headers["Content-Type"] = "application/x-www-form-urlencoded"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | None:
    if response.status_code == 303:
        return None

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: UsersPageActBody,
) -> Response[Any]:
    """One act of the users page, named by action, run as <user>-admin.

     set_quota (user, quota, limit; empty limit returns to the default) journals web.quota_set;
    disable_user journals web.user_disabled and cuts off every agent key of the person; close_deletion
    disables the person and journals web.deletion_closed (ADR-0068 d5, d6, d8). Proposed.

    Args:
        body (UsersPageActBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: UsersPageActBody,
) -> Response[Any]:
    """One act of the users page, named by action, run as <user>-admin.

     set_quota (user, quota, limit; empty limit returns to the default) journals web.quota_set;
    disable_user journals web.user_disabled and cuts off every agent key of the person; close_deletion
    disables the person and journals web.deletion_closed (ADR-0068 d5, d6, d8). Proposed.

    Args:
        body (UsersPageActBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)
