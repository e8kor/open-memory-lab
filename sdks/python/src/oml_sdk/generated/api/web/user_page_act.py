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
    body: UserPageActBody,
) -> Response[Any | str]:
    """One act of the user page, named by action.

     Requires a session, csrf_token and the same-origin check (oauth.origin_refused; request.validation
    with details.field csrf_token). action is one of issue_key, connect_key, connect_code, cut_off,
    review_next, verdict, sign_out; review_next and verdict run as the session's <user>-review key,
    which holds dreamer and reader on the person's own root from the first sign-in, R2 checked on that
    key and the lineage closure; every other action runs as <user>-web; no field names who acts, and a
    field the action does not define is request.validation (ADR-0045 d6, d9). create_workarea,
    workarea_mode, workarea_transition and workarea_ratify are the Workareas view's acts, which oml-web
    runs at /web/workareas over the API: the lead runs none of them and answers 303 /web/workareas
    (ADR-0070 d7 (b); since v0.6.4). An issued key's value or connect code is shown once on the
    answering page, never in a redirect. Since v0.5.0.

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
    """One act of the user page, named by action.

     Requires a session, csrf_token and the same-origin check (oauth.origin_refused; request.validation
    with details.field csrf_token). action is one of issue_key, connect_key, connect_code, cut_off,
    review_next, verdict, sign_out; review_next and verdict run as the session's <user>-review key,
    which holds dreamer and reader on the person's own root from the first sign-in, R2 checked on that
    key and the lineage closure; every other action runs as <user>-web; no field names who acts, and a
    field the action does not define is request.validation (ADR-0045 d6, d9). create_workarea,
    workarea_mode, workarea_transition and workarea_ratify are the Workareas view's acts, which oml-web
    runs at /web/workareas over the API: the lead runs none of them and answers 303 /web/workareas
    (ADR-0070 d7 (b); since v0.6.4). An issued key's value or connect code is shown once on the
    answering page, never in a redirect. Since v0.5.0.

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
    """One act of the user page, named by action.

     Requires a session, csrf_token and the same-origin check (oauth.origin_refused; request.validation
    with details.field csrf_token). action is one of issue_key, connect_key, connect_code, cut_off,
    review_next, verdict, sign_out; review_next and verdict run as the session's <user>-review key,
    which holds dreamer and reader on the person's own root from the first sign-in, R2 checked on that
    key and the lineage closure; every other action runs as <user>-web; no field names who acts, and a
    field the action does not define is request.validation (ADR-0045 d6, d9). create_workarea,
    workarea_mode, workarea_transition and workarea_ratify are the Workareas view's acts, which oml-web
    runs at /web/workareas over the API: the lead runs none of them and answers 303 /web/workareas
    (ADR-0070 d7 (b); since v0.6.4). An issued key's value or connect code is shown once on the
    answering page, never in a redirect. Since v0.5.0.

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
    """One act of the user page, named by action.

     Requires a session, csrf_token and the same-origin check (oauth.origin_refused; request.validation
    with details.field csrf_token). action is one of issue_key, connect_key, connect_code, cut_off,
    review_next, verdict, sign_out; review_next and verdict run as the session's <user>-review key,
    which holds dreamer and reader on the person's own root from the first sign-in, R2 checked on that
    key and the lineage closure; every other action runs as <user>-web; no field names who acts, and a
    field the action does not define is request.validation (ADR-0045 d6, d9). create_workarea,
    workarea_mode, workarea_transition and workarea_ratify are the Workareas view's acts, which oml-web
    runs at /web/workareas over the API: the lead runs none of them and answers 303 /web/workareas
    (ADR-0070 d7 (b); since v0.6.4). An issued key's value or connect code is shown once on the
    answering page, never in a redirect. Since v0.5.0.

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
