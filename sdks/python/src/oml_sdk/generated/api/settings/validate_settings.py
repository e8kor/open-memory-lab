from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.settings_document import SettingsDocument
from ...models.settings_validation import SettingsValidation
from ...types import Response


def _get_kwargs(
    *,
    body: SettingsDocument,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/settings/validate",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | SettingsValidation:
    if response.status_code == 200:
        response_200 = SettingsValidation.from_dict(response.json())

        return response_200

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | SettingsValidation]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: SettingsDocument,
) -> Response[ErrorEnvelope | SettingsValidation]:
    """Server-side validation without applying; a retired settings key is refused by name, with its
    replacement.

     Requires: admin key role. Read-shaped POST: no Idempotency-Key.

    Args:
        body (SettingsDocument): The settings document; its schema is not fixed by this design —
            the contract is show/validate/apply and the retired-key refusal.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SettingsValidation]
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
    body: SettingsDocument,
) -> ErrorEnvelope | SettingsValidation | None:
    """Server-side validation without applying; a retired settings key is refused by name, with its
    replacement.

     Requires: admin key role. Read-shaped POST: no Idempotency-Key.

    Args:
        body (SettingsDocument): The settings document; its schema is not fixed by this design —
            the contract is show/validate/apply and the retired-key refusal.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SettingsValidation
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: SettingsDocument,
) -> Response[ErrorEnvelope | SettingsValidation]:
    """Server-side validation without applying; a retired settings key is refused by name, with its
    replacement.

     Requires: admin key role. Read-shaped POST: no Idempotency-Key.

    Args:
        body (SettingsDocument): The settings document; its schema is not fixed by this design —
            the contract is show/validate/apply and the retired-key refusal.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SettingsValidation]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: SettingsDocument,
) -> ErrorEnvelope | SettingsValidation | None:
    """Server-side validation without applying; a retired settings key is refused by name, with its
    replacement.

     Requires: admin key role. Read-shaped POST: no Idempotency-Key.

    Args:
        body (SettingsDocument): The settings document; its schema is not fixed by this design —
            the contract is show/validate/apply and the retired-key refusal.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SettingsValidation
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
