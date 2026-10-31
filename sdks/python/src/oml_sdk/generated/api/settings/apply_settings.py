from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.act_result import ActResult
from ...models.error_envelope import ErrorEnvelope
from ...models.settings_document import SettingsDocument
from ...types import Response


def _get_kwargs(
    *,
    body: SettingsDocument,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/settings/apply",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ActResult | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = ActResult.from_dict(response.json())

        return response_200

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    if response.status_code == 422:
        response_422 = ErrorEnvelope.from_dict(response.json())

        return response_422

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ActResult | ErrorEnvelope]:
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
    idempotency_key: str,
) -> Response[ActResult | ErrorEnvelope]:
    """Validate, journal, apply; refused whole on any validation error, and there is no force flag.

     Requires: admin key role. Journals system.settings_applied.

    Args:
        idempotency_key (str):
        body (SettingsDocument): The settings document; its schema is not fixed by this design —
            the contract is show/validate/apply and the retired-key refusal.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ActResult | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        body=body,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: SettingsDocument,
    idempotency_key: str,
) -> ActResult | ErrorEnvelope | None:
    """Validate, journal, apply; refused whole on any validation error, and there is no force flag.

     Requires: admin key role. Journals system.settings_applied.

    Args:
        idempotency_key (str):
        body (SettingsDocument): The settings document; its schema is not fixed by this design —
            the contract is show/validate/apply and the retired-key refusal.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ActResult | ErrorEnvelope
    """

    return sync_detailed(
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: SettingsDocument,
    idempotency_key: str,
) -> Response[ActResult | ErrorEnvelope]:
    """Validate, journal, apply; refused whole on any validation error, and there is no force flag.

     Requires: admin key role. Journals system.settings_applied.

    Args:
        idempotency_key (str):
        body (SettingsDocument): The settings document; its schema is not fixed by this design —
            the contract is show/validate/apply and the retired-key refusal.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ActResult | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        body=body,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: SettingsDocument,
    idempotency_key: str,
) -> ActResult | ErrorEnvelope | None:
    """Validate, journal, apply; refused whole on any validation error, and there is no force flag.

     Requires: admin key role. Journals system.settings_applied.

    Args:
        idempotency_key (str):
        body (SettingsDocument): The settings document; its schema is not fixed by this design —
            the contract is show/validate/apply and the retired-key refusal.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ActResult | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
