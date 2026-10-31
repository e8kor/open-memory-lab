from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.term_mapping_page import TermMappingPage
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    term: str,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["term"] = term

    params["limit"] = limit

    params["cursor"] = cursor

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/vocab/mappings",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | TermMappingPage:
    if response.status_code == 200:
        response_200 = TermMappingPage.from_dict(response.json())

        return response_200

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | TermMappingPage]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    term: str,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> Response[ErrorEnvelope | TermMappingPage]:
    """Ratified SSSOM-shaped mappings touching a term; both endpoints persist — a mapping never merges.

     Requires: reader effective access on the queried term's vocabulary; mapping rows outside the key's
    read scope are absent (fail closed).

    Args:
        term (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once
            minted (first promotion).
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | TermMappingPage]
    """

    kwargs = _get_kwargs(
        term=term,
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
    term: str,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> ErrorEnvelope | TermMappingPage | None:
    """Ratified SSSOM-shaped mappings touching a term; both endpoints persist — a mapping never merges.

     Requires: reader effective access on the queried term's vocabulary; mapping rows outside the key's
    read scope are absent (fail closed).

    Args:
        term (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once
            minted (first promotion).
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | TermMappingPage
    """

    return sync_detailed(
        client=client,
        term=term,
        limit=limit,
        cursor=cursor,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    term: str,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> Response[ErrorEnvelope | TermMappingPage]:
    """Ratified SSSOM-shaped mappings touching a term; both endpoints persist — a mapping never merges.

     Requires: reader effective access on the queried term's vocabulary; mapping rows outside the key's
    read scope are absent (fail closed).

    Args:
        term (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once
            minted (first promotion).
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | TermMappingPage]
    """

    kwargs = _get_kwargs(
        term=term,
        limit=limit,
        cursor=cursor,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    term: str,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> ErrorEnvelope | TermMappingPage | None:
    """Ratified SSSOM-shaped mappings touching a term; both endpoints persist — a mapping never merges.

     Requires: reader effective access on the queried term's vocabulary; mapping rows outside the key's
    read scope are absent (fail closed).

    Args:
        term (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once
            minted (first promotion).
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | TermMappingPage
    """

    return (
        await asyncio_detailed(
            client=client,
            term=term,
            limit=limit,
            cursor=cursor,
        )
    ).parsed
