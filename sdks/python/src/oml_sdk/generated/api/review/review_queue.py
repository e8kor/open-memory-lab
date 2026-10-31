from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.review_queue_page import ReviewQueuePage
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workarea: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["workarea"] = workarea

    params["limit"] = limit

    params["cursor"] = cursor

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/review/queue",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | ReviewQueuePage:
    if response.status_code == 200:
        response_200 = ReviewQueuePage.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | ReviewQueuePage]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ReviewQueuePage]:
    """Held-item counts and oldest ages per workarea in the reviewer's scope.

     Requires: dreamer(review) effective access. A held_by: service key is refused with
    access.verb_ceiling before any lookup (ADR-0025 d1).

    Args:
        workarea (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ReviewQueuePage]
    """

    kwargs = _get_kwargs(
        workarea=workarea,
        limit=limit,
        cursor=cursor,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> ErrorEnvelope | ReviewQueuePage | None:
    """Held-item counts and oldest ages per workarea in the reviewer's scope.

     Requires: dreamer(review) effective access. A held_by: service key is refused with
    access.verb_ceiling before any lookup (ADR-0025 d1).

    Args:
        workarea (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ReviewQueuePage
    """

    return sync_detailed(
        client=client,
        workarea=workarea,
        limit=limit,
        cursor=cursor,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ReviewQueuePage]:
    """Held-item counts and oldest ages per workarea in the reviewer's scope.

     Requires: dreamer(review) effective access. A held_by: service key is refused with
    access.verb_ceiling before any lookup (ADR-0025 d1).

    Args:
        workarea (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ReviewQueuePage]
    """

    kwargs = _get_kwargs(
        workarea=workarea,
        limit=limit,
        cursor=cursor,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> ErrorEnvelope | ReviewQueuePage | None:
    """Held-item counts and oldest ages per workarea in the reviewer's scope.

     Requires: dreamer(review) effective access. A held_by: service key is refused with
    access.verb_ceiling before any lookup (ADR-0025 d1).

    Args:
        workarea (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ReviewQueuePage
    """

    return (
        await asyncio_detailed(
            client=client,
            workarea=workarea,
            limit=limit,
            cursor=cursor,
        )
    ).parsed
