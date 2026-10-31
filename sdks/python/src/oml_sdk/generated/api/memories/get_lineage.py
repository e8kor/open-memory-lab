from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.lineage import Lineage
from ...types import Response


def _get_kwargs(
    artifact_id: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/memories/{artifact_id}/lineage".format(
            artifact_id=quote(str(artifact_id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | Lineage:
    if response.status_code == 200:
        response_200 = Lineage.from_dict(response.json())

        return response_200

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | Lineage]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorEnvelope | Lineage]:
    """The derivation closure, pending and ratified edges, serialized as stored.

     Requires: reader effective access. Serialize, don't compute (D13); out-of-scope nodes are elided to
    redacted ids.

    Args:
        artifact_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Lineage]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorEnvelope | Lineage | None:
    """The derivation closure, pending and ratified edges, serialized as stored.

     Requires: reader effective access. Serialize, don't compute (D13); out-of-scope nodes are elided to
    redacted ids.

    Args:
        artifact_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Lineage
    """

    return sync_detailed(
        artifact_id=artifact_id,
        client=client,
    ).parsed


async def asyncio_detailed(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorEnvelope | Lineage]:
    """The derivation closure, pending and ratified edges, serialized as stored.

     Requires: reader effective access. Serialize, don't compute (D13); out-of-scope nodes are elided to
    redacted ids.

    Args:
        artifact_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Lineage]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorEnvelope | Lineage | None:
    """The derivation closure, pending and ratified edges, serialized as stored.

     Requires: reader effective access. Serialize, don't compute (D13); out-of-scope nodes are elided to
    redacted ids.

    Args:
        artifact_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Lineage
    """

    return (
        await asyncio_detailed(
            artifact_id=artifact_id,
            client=client,
        )
    ).parsed
