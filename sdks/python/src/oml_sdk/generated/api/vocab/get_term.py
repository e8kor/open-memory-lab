from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.vocab_term import VocabTerm
from ...types import Response


def _get_kwargs(
    curie: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/vocab/terms/{curie}".format(
            curie=quote(str(curie), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | VocabTerm:
    if response.status_code == 200:
        response_200 = VocabTerm.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | VocabTerm]:
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
) -> Response[ErrorEnvelope | VocabTerm]:
    """One term record — status, successors, the releases its tip appears in.

     Requires: reader effective access on the owning vocabulary workarea. Unknown and out-of-scope CURIEs
    answer 404 access.not_found, indistinguishable (fail closed).

    Args:
        curie (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once
            minted (first promotion).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | VocabTerm]
    """

    kwargs = _get_kwargs(
        curie=curie,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    curie: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorEnvelope | VocabTerm | None:
    """One term record — status, successors, the releases its tip appears in.

     Requires: reader effective access on the owning vocabulary workarea. Unknown and out-of-scope CURIEs
    answer 404 access.not_found, indistinguishable (fail closed).

    Args:
        curie (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once
            minted (first promotion).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | VocabTerm
    """

    return sync_detailed(
        curie=curie,
        client=client,
    ).parsed


async def asyncio_detailed(
    curie: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorEnvelope | VocabTerm]:
    """One term record — status, successors, the releases its tip appears in.

     Requires: reader effective access on the owning vocabulary workarea. Unknown and out-of-scope CURIEs
    answer 404 access.not_found, indistinguishable (fail closed).

    Args:
        curie (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once
            minted (first promotion).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | VocabTerm]
    """

    kwargs = _get_kwargs(
        curie=curie,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    curie: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorEnvelope | VocabTerm | None:
    """One term record — status, successors, the releases its tip appears in.

     Requires: reader effective access on the owning vocabulary workarea. Unknown and out-of-scope CURIEs
    answer 404 access.not_found, indistinguishable (fail closed).

    Args:
        curie (str): PREFIX:localname over the tenant's journalled prefix registry; permanent once
            minted (first promotion).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | VocabTerm
    """

    return (
        await asyncio_detailed(
            curie=curie,
            client=client,
        )
    ).parsed
