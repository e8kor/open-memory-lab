from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.settings_view import SettingsView
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    effective: bool | Unset = False,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["effective"] = effective

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/settings",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | SettingsView:
    if response.status_code == 200:
        response_200 = SettingsView.from_dict(response.json())

        return response_200

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | SettingsView]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    effective: bool | Unset = False,
) -> Response[ErrorEnvelope | SettingsView]:
    """The configured document as last applied; effective=true adds running values with every divergence
    marked per key.

     Requires: admin key role.

    Args:
        effective (bool | Unset):  Default: False.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SettingsView]
    """

    kwargs = _get_kwargs(
        effective=effective,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    effective: bool | Unset = False,
) -> ErrorEnvelope | SettingsView | None:
    """The configured document as last applied; effective=true adds running values with every divergence
    marked per key.

     Requires: admin key role.

    Args:
        effective (bool | Unset):  Default: False.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SettingsView
    """

    return sync_detailed(
        client=client,
        effective=effective,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    effective: bool | Unset = False,
) -> Response[ErrorEnvelope | SettingsView]:
    """The configured document as last applied; effective=true adds running values with every divergence
    marked per key.

     Requires: admin key role.

    Args:
        effective (bool | Unset):  Default: False.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SettingsView]
    """

    kwargs = _get_kwargs(
        effective=effective,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    effective: bool | Unset = False,
) -> ErrorEnvelope | SettingsView | None:
    """The configured document as last applied; effective=true adds running values with every divergence
    marked per key.

     Requires: admin key role.

    Args:
        effective (bool | Unset):  Default: False.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SettingsView
    """

    return (
        await asyncio_detailed(
            client=client,
            effective=effective,
        )
    ).parsed
