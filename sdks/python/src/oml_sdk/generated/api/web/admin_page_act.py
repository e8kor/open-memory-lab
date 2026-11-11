from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.admin_page_act_body import AdminPageActBody
from ...types import Response


def _get_kwargs(
    *,
    body: AdminPageActBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/web/admin",
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
    body: AdminPageActBody,
) -> Response[Any | str]:
    """One act of the admin page, named by action, run as the capacity it needs.

     Requires an admin session, csrf_token and the same-origin check. allow_email, remove_email,
    set_admin, grant_review and grant_curation run as <user>-admin; review_next, verdict and supersede
    (dreamer propose) as <user>-review; curate and supersede of the curator's own universe claims as
    <user>-curate; demote, since v0.6.0, as <user>-admin. R2 is enforced on the reviewing key and the
    lineage closure; the admin key is never offered a verdict (ADR-0045 d10, d11, d12). workarea_mode,
    workarea_transition and workarea_ratify are the Workareas view's acts, which oml-web runs at
    /web/admin/workareas over the API: the lead runs none of them and answers 303 /web/admin/workareas
    (ADR-0070 d7 (b); since v0.6.4). Since v0.5.0.

    Args:
        body (AdminPageActBody):

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
    body: AdminPageActBody,
) -> Any | str | None:
    """One act of the admin page, named by action, run as the capacity it needs.

     Requires an admin session, csrf_token and the same-origin check. allow_email, remove_email,
    set_admin, grant_review and grant_curation run as <user>-admin; review_next, verdict and supersede
    (dreamer propose) as <user>-review; curate and supersede of the curator's own universe claims as
    <user>-curate; demote, since v0.6.0, as <user>-admin. R2 is enforced on the reviewing key and the
    lineage closure; the admin key is never offered a verdict (ADR-0045 d10, d11, d12). workarea_mode,
    workarea_transition and workarea_ratify are the Workareas view's acts, which oml-web runs at
    /web/admin/workareas over the API: the lead runs none of them and answers 303 /web/admin/workareas
    (ADR-0070 d7 (b); since v0.6.4). Since v0.5.0.

    Args:
        body (AdminPageActBody):

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
    body: AdminPageActBody,
) -> Response[Any | str]:
    """One act of the admin page, named by action, run as the capacity it needs.

     Requires an admin session, csrf_token and the same-origin check. allow_email, remove_email,
    set_admin, grant_review and grant_curation run as <user>-admin; review_next, verdict and supersede
    (dreamer propose) as <user>-review; curate and supersede of the curator's own universe claims as
    <user>-curate; demote, since v0.6.0, as <user>-admin. R2 is enforced on the reviewing key and the
    lineage closure; the admin key is never offered a verdict (ADR-0045 d10, d11, d12). workarea_mode,
    workarea_transition and workarea_ratify are the Workareas view's acts, which oml-web runs at
    /web/admin/workareas over the API: the lead runs none of them and answers 303 /web/admin/workareas
    (ADR-0070 d7 (b); since v0.6.4). Since v0.5.0.

    Args:
        body (AdminPageActBody):

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
    body: AdminPageActBody,
) -> Any | str | None:
    """One act of the admin page, named by action, run as the capacity it needs.

     Requires an admin session, csrf_token and the same-origin check. allow_email, remove_email,
    set_admin, grant_review and grant_curation run as <user>-admin; review_next, verdict and supersede
    (dreamer propose) as <user>-review; curate and supersede of the curator's own universe claims as
    <user>-curate; demote, since v0.6.0, as <user>-admin. R2 is enforced on the reviewing key and the
    lineage closure; the admin key is never offered a verdict (ADR-0045 d10, d11, d12). workarea_mode,
    workarea_transition and workarea_ratify are the Workareas view's acts, which oml-web runs at
    /web/admin/workareas over the API: the lead runs none of them and answers 303 /web/admin/workareas
    (ADR-0070 d7 (b); since v0.6.4). Since v0.5.0.

    Args:
        body (AdminPageActBody):

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
