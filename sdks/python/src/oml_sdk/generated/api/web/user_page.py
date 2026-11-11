from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.user_page_view import UserPageView
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    view: UserPageView | Unset = UNSET,
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
        "url": "/web",
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
    view: UserPageView | Unset = UNSET,
    q: str | Unset = UNSET,
    memory: str | Unset = UNSET,
) -> Response[Any | str]:
    """The sign-in page without a session; the user page with one.

     Authenticated by the __Host-oml_session cookie alone; an Authorization header is refused with
    access.verb_ceiling, details.reason web_is_human_only (ADR-0045 d7). Views by query: view (keys,
    setup, memories, workarea, review), q, memory, workarea. Served with the connect page's headers
    (ADR-0045 d8). Routed only on the lead and only where oauth.issuer and a sign-in provider are set.
    Since v0.5.0.

    Args:
        view (UserPageView | Unset):
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
    view: UserPageView | Unset = UNSET,
    q: str | Unset = UNSET,
    memory: str | Unset = UNSET,
) -> Any | str | None:
    """The sign-in page without a session; the user page with one.

     Authenticated by the __Host-oml_session cookie alone; an Authorization header is refused with
    access.verb_ceiling, details.reason web_is_human_only (ADR-0045 d7). Views by query: view (keys,
    setup, memories, workarea, review), q, memory, workarea. Served with the connect page's headers
    (ADR-0045 d8). Routed only on the lead and only where oauth.issuer and a sign-in provider are set.
    Since v0.5.0.

    Args:
        view (UserPageView | Unset):
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
    view: UserPageView | Unset = UNSET,
    q: str | Unset = UNSET,
    memory: str | Unset = UNSET,
) -> Response[Any | str]:
    """The sign-in page without a session; the user page with one.

     Authenticated by the __Host-oml_session cookie alone; an Authorization header is refused with
    access.verb_ceiling, details.reason web_is_human_only (ADR-0045 d7). Views by query: view (keys,
    setup, memories, workarea, review), q, memory, workarea. Served with the connect page's headers
    (ADR-0045 d8). Routed only on the lead and only where oauth.issuer and a sign-in provider are set.
    Since v0.5.0.

    Args:
        view (UserPageView | Unset):
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
    view: UserPageView | Unset = UNSET,
    q: str | Unset = UNSET,
    memory: str | Unset = UNSET,
) -> Any | str | None:
    """The sign-in page without a session; the user page with one.

     Authenticated by the __Host-oml_session cookie alone; an Authorization header is refused with
    access.verb_ceiling, details.reason web_is_human_only (ADR-0045 d7). Views by query: view (keys,
    setup, memories, workarea, review), q, memory, workarea. Served with the connect page's headers
    (ADR-0045 d8). Routed only on the lead and only where oauth.issuer and a sign-in provider are set.
    Since v0.5.0.

    Args:
        view (UserPageView | Unset):
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
