from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.user_page_act_body import UserPageActBody
from ...types import Response


def _get_kwargs(
    *,
    body: UserPageActBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/web",
    }

    _kwargs["data"] = body.to_dict()
    headers["Content-Type"] = "application/x-www-form-urlencoded"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | str | None:
    if response.status_code == 303:
        response_303 = cast(Any, None)
        return response_303

    if response.status_code == 400:
        response_400 = response.text
        return response_400

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
    body: UserPageActBody,
) -> Response[Any | str]:
    """Sign out, the one act the lead keeps.

     Requires a session, csrf_token and the same-origin check (oauth.origin_refused; request.validation
    with details.field csrf_token). action is sign_out, which takes no other field; every other act is
    oml-web's at its view's own path, and an old form's action is request.validation with details.field
    action, running nothing (ADR-0070 d6, d7 (b); the moved acts' 303s deleted in v0.7.1). Since v0.5.0.

    Args:
        body (UserPageActBody):

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
    body: UserPageActBody,
) -> Any | str | None:
    """Sign out, the one act the lead keeps.

     Requires a session, csrf_token and the same-origin check (oauth.origin_refused; request.validation
    with details.field csrf_token). action is sign_out, which takes no other field; every other act is
    oml-web's at its view's own path, and an old form's action is request.validation with details.field
    action, running nothing (ADR-0070 d6, d7 (b); the moved acts' 303s deleted in v0.7.1). Since v0.5.0.

    Args:
        body (UserPageActBody):

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
    body: UserPageActBody,
) -> Response[Any | str]:
    """Sign out, the one act the lead keeps.

     Requires a session, csrf_token and the same-origin check (oauth.origin_refused; request.validation
    with details.field csrf_token). action is sign_out, which takes no other field; every other act is
    oml-web's at its view's own path, and an old form's action is request.validation with details.field
    action, running nothing (ADR-0070 d6, d7 (b); the moved acts' 303s deleted in v0.7.1). Since v0.5.0.

    Args:
        body (UserPageActBody):

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
    body: UserPageActBody,
) -> Any | str | None:
    """Sign out, the one act the lead keeps.

     Requires a session, csrf_token and the same-origin check (oauth.origin_refused; request.validation
    with details.field csrf_token). action is sign_out, which takes no other field; every other act is
    oml-web's at its view's own path, and an old form's action is request.validation with details.field
    action, running nothing (ADR-0070 d6, d7 (b); the moved acts' 303s deleted in v0.7.1). Since v0.5.0.

    Args:
        body (UserPageActBody):

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
