from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.get_term_hierarchy_direction import GetTermHierarchyDirection
from ...models.vocab_hierarchy_page import VocabHierarchyPage
from ...types import UNSET, Response, Unset


def _get_kwargs(
    curie: str,
    *,
    direction: GetTermHierarchyDirection,
    release: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    json_direction = direction.value
    params["direction"] = json_direction

    params["release"] = release

    params["limit"] = limit

    params["cursor"] = cursor

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/vocab/terms/{curie}/hierarchy".format(
            curie=quote(str(curie), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | VocabHierarchyPage:
    if response.status_code == 200:
        response_200 = VocabHierarchyPage.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | VocabHierarchyPage]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    curie: str,
    *,
    client: AuthenticatedClient | Client,
    direction: GetTermHierarchyDirection,
    release: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> Response[ErrorEnvelope | VocabHierarchyPage]:
    """Parents, children, ancestors or descendants over ratified is_a — a lookup over the per-release
    closure, never a traversal.

     Requires: reader effective access on the owning vocabulary workarea. Closure rows are materialized
    at release ratification (serialize, don't compute — ADR-0021); release defaults to the owning
    vocabulary's latest ratified release.

    Args:
        curie (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once
            minted (first promotion).
        direction (GetTermHierarchyDirection):
        release (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | VocabHierarchyPage]
    """

    kwargs = _get_kwargs(
        curie=curie,
        direction=direction,
        release=release,
        limit=limit,
        cursor=cursor,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    curie: str,
    *,
    client: AuthenticatedClient | Client,
    direction: GetTermHierarchyDirection,
    release: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> ErrorEnvelope | VocabHierarchyPage | None:
    """Parents, children, ancestors or descendants over ratified is_a — a lookup over the per-release
    closure, never a traversal.

     Requires: reader effective access on the owning vocabulary workarea. Closure rows are materialized
    at release ratification (serialize, don't compute — ADR-0021); release defaults to the owning
    vocabulary's latest ratified release.

    Args:
        curie (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once
            minted (first promotion).
        direction (GetTermHierarchyDirection):
        release (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | VocabHierarchyPage
    """

    return sync_detailed(
        curie=curie,
        client=client,
        direction=direction,
        release=release,
        limit=limit,
        cursor=cursor,
    ).parsed


async def asyncio_detailed(
    curie: str,
    *,
    client: AuthenticatedClient | Client,
    direction: GetTermHierarchyDirection,
    release: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> Response[ErrorEnvelope | VocabHierarchyPage]:
    """Parents, children, ancestors or descendants over ratified is_a — a lookup over the per-release
    closure, never a traversal.

     Requires: reader effective access on the owning vocabulary workarea. Closure rows are materialized
    at release ratification (serialize, don't compute — ADR-0021); release defaults to the owning
    vocabulary's latest ratified release.

    Args:
        curie (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once
            minted (first promotion).
        direction (GetTermHierarchyDirection):
        release (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | VocabHierarchyPage]
    """

    kwargs = _get_kwargs(
        curie=curie,
        direction=direction,
        release=release,
        limit=limit,
        cursor=cursor,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    curie: str,
    *,
    client: AuthenticatedClient | Client,
    direction: GetTermHierarchyDirection,
    release: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> ErrorEnvelope | VocabHierarchyPage | None:
    """Parents, children, ancestors or descendants over ratified is_a — a lookup over the per-release
    closure, never a traversal.

     Requires: reader effective access on the owning vocabulary workarea. Closure rows are materialized
    at release ratification (serialize, don't compute — ADR-0021); release defaults to the owning
    vocabulary's latest ratified release.

    Args:
        curie (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once
            minted (first promotion).
        direction (GetTermHierarchyDirection):
        release (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | VocabHierarchyPage
    """

    return (
        await asyncio_detailed(
            curie=curie,
            client=client,
            direction=direction,
            release=release,
            limit=limit,
            cursor=cursor,
        )
    ).parsed
