from http import HTTPStatus
from io import BytesIO
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, File, Response, Unset


def _get_kwargs(
    digest: str,
    *,
    serve_id: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["serve_id"] = serve_id

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/attachments/{digest}".format(
            digest=quote(str(digest), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | File:
    if response.status_code == 200:
        response_200 = File(payload=BytesIO(response.content))

        return response_200

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    if response.status_code == 410:
        response_410 = ErrorEnvelope.from_dict(response.json())

        return response_410

    if response.status_code == 429:
        response_429 = ErrorEnvelope.from_dict(response.json())

        return response_429

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | File]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    digest: str,
    *,
    client: AuthenticatedClient | Client,
    serve_id: str | Unset = UNSET,
) -> Response[ErrorEnvelope | File]:
    """Fetch attachment bytes, decrypted on the serving path, plaintext digest verified before serving.

     Requires: reader access to a promoted claim binding the digest; the author for own held; a reviewer
    under a valid serve_id. Logged as a read.

    Args:
        digest (str): SHA-256 of plaintext unless a field says ciphertext.
        serve_id (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | File]
    """

    kwargs = _get_kwargs(
        digest=digest,
        serve_id=serve_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    digest: str,
    *,
    client: AuthenticatedClient | Client,
    serve_id: str | Unset = UNSET,
) -> ErrorEnvelope | File | None:
    """Fetch attachment bytes, decrypted on the serving path, plaintext digest verified before serving.

     Requires: reader access to a promoted claim binding the digest; the author for own held; a reviewer
    under a valid serve_id. Logged as a read.

    Args:
        digest (str): SHA-256 of plaintext unless a field says ciphertext.
        serve_id (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | File
    """

    return sync_detailed(
        digest=digest,
        client=client,
        serve_id=serve_id,
    ).parsed


async def asyncio_detailed(
    digest: str,
    *,
    client: AuthenticatedClient | Client,
    serve_id: str | Unset = UNSET,
) -> Response[ErrorEnvelope | File]:
    """Fetch attachment bytes, decrypted on the serving path, plaintext digest verified before serving.

     Requires: reader access to a promoted claim binding the digest; the author for own held; a reviewer
    under a valid serve_id. Logged as a read.

    Args:
        digest (str): SHA-256 of plaintext unless a field says ciphertext.
        serve_id (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | File]
    """

    kwargs = _get_kwargs(
        digest=digest,
        serve_id=serve_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    digest: str,
    *,
    client: AuthenticatedClient | Client,
    serve_id: str | Unset = UNSET,
) -> ErrorEnvelope | File | None:
    """Fetch attachment bytes, decrypted on the serving path, plaintext digest verified before serving.

     Requires: reader access to a promoted claim binding the digest; the author for own held; a reviewer
    under a valid serve_id. Logged as a read.

    Args:
        digest (str): SHA-256 of plaintext unless a field says ciphertext.
        serve_id (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | File
    """

    return (
        await asyncio_detailed(
            digest=digest,
            client=client,
            serve_id=serve_id,
        )
    ).parsed
