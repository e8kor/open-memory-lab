from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.effective_access import EffectiveAccess
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    key_id: str,
    *,
    workarea: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["workarea"] = workarea

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/keys/{key_id}/effective-access".format(
            key_id=quote(str(key_id), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> EffectiveAccess | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = EffectiveAccess.from_dict(response.json())

        return response_200

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[EffectiveAccess | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    key_id: str,
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
) -> Response[EffectiveAccess | ErrorEnvelope]:
    """The resolved answer — what this key may do where, and which deny produced each hole.

     Requires: admin key role. Reproducible from journalled grants by construction (permission epoch).

    Args:
        key_id (str):
        workarea (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[EffectiveAccess | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        key_id=key_id,
        workarea=workarea,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    key_id: str,
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
) -> EffectiveAccess | ErrorEnvelope | None:
    """The resolved answer — what this key may do where, and which deny produced each hole.

     Requires: admin key role. Reproducible from journalled grants by construction (permission epoch).

    Args:
        key_id (str):
        workarea (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        EffectiveAccess | ErrorEnvelope
    """

    return sync_detailed(
        key_id=key_id,
        client=client,
        workarea=workarea,
    ).parsed


async def asyncio_detailed(
    key_id: str,
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
) -> Response[EffectiveAccess | ErrorEnvelope]:
    """The resolved answer — what this key may do where, and which deny produced each hole.

     Requires: admin key role. Reproducible from journalled grants by construction (permission epoch).

    Args:
        key_id (str):
        workarea (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[EffectiveAccess | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        key_id=key_id,
        workarea=workarea,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    key_id: str,
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
) -> EffectiveAccess | ErrorEnvelope | None:
    """The resolved answer — what this key may do where, and which deny produced each hole.

     Requires: admin key role. Reproducible from journalled grants by construction (permission epoch).

    Args:
        key_id (str):
        workarea (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        EffectiveAccess | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            key_id=key_id,
            client=client,
            workarea=workarea,
        )
    ).parsed
