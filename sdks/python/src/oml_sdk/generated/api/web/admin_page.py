from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.admin_page_view import AdminPageView
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    view: AdminPageView | Unset = UNSET,
    q: str | Unset = UNSET,
    memory: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    json_view: str | Unset = UNSET
    if not isinstance(view, Unset):
        json_view = view.value

    params["view"] = json_view

    params["q"] = q

    params["memory"] = memory

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/web/admin",
        "params": params,
    }

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

    if response.status_code == 404:
        response_404 = response.text
        return response_404

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
    view: AdminPageView | Unset = UNSET,
    q: str | Unset = UNSET,
    memory: str | Unset = UNSET,
) -> Response[Any | str]:
    """The admin page, for a session whose allowlist entry is an admin's.

     Every view is oml-web's: the lead answers 303 to the view's own path, /web/admin/people when none is
    named, before reading a session and carrying nothing from the request but, for one memory, its id
    (ADR-0070 d7 (b); the overview since v0.7.0). The admin read (POST /v2/admin/memories/search, GET
    /v2/admin/memories/{artifact_id}) as <user>-admin serves oml-web's memory views, each read
    journalled (ADR-0045 d10, d21). Since v0.5.0.

    Args:
        view (AdminPageView | Unset):
        q (str | Unset):
        memory (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs(
        view=view,
        q=q,
        memory=memory,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    view: AdminPageView | Unset = UNSET,
    q: str | Unset = UNSET,
    memory: str | Unset = UNSET,
) -> Any | str | None:
    """The admin page, for a session whose allowlist entry is an admin's.

     Every view is oml-web's: the lead answers 303 to the view's own path, /web/admin/people when none is
    named, before reading a session and carrying nothing from the request but, for one memory, its id
    (ADR-0070 d7 (b); the overview since v0.7.0). The admin read (POST /v2/admin/memories/search, GET
    /v2/admin/memories/{artifact_id}) as <user>-admin serves oml-web's memory views, each read
    journalled (ADR-0045 d10, d21). Since v0.5.0.

    Args:
        view (AdminPageView | Unset):
        q (str | Unset):
        memory (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
    """

    return sync_detailed(
        client=client,
        view=view,
        q=q,
        memory=memory,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    view: AdminPageView | Unset = UNSET,
    q: str | Unset = UNSET,
    memory: str | Unset = UNSET,
) -> Response[Any | str]:
    """The admin page, for a session whose allowlist entry is an admin's.

     Every view is oml-web's: the lead answers 303 to the view's own path, /web/admin/people when none is
    named, before reading a session and carrying nothing from the request but, for one memory, its id
    (ADR-0070 d7 (b); the overview since v0.7.0). The admin read (POST /v2/admin/memories/search, GET
    /v2/admin/memories/{artifact_id}) as <user>-admin serves oml-web's memory views, each read
    journalled (ADR-0045 d10, d21). Since v0.5.0.

    Args:
        view (AdminPageView | Unset):
        q (str | Unset):
        memory (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs(
        view=view,
        q=q,
        memory=memory,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    view: AdminPageView | Unset = UNSET,
    q: str | Unset = UNSET,
    memory: str | Unset = UNSET,
) -> Any | str | None:
    """The admin page, for a session whose allowlist entry is an admin's.

     Every view is oml-web's: the lead answers 303 to the view's own path, /web/admin/people when none is
    named, before reading a session and carrying nothing from the request but, for one memory, its id
    (ADR-0070 d7 (b); the overview since v0.7.0). The admin read (POST /v2/admin/memories/search, GET
    /v2/admin/memories/{artifact_id}) as <user>-admin serves oml-web's memory views, each read
    journalled (ADR-0045 d10, d21). Since v0.5.0.

    Args:
        view (AdminPageView | Unset):
        q (str | Unset):
        memory (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
    """

    return (
        await asyncio_detailed(
            client=client,
            view=view,
            q=q,
            memory=memory,
        )
    ).parsed
