from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.bootstrap_claim_body import BootstrapClaimBody
from ...models.error_envelope import ErrorEnvelope
from ...models.key_issued import KeyIssued
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: BootstrapClaimBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/bootstrap/claim",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | KeyIssued:
    if response.status_code == 201:
        response_201 = KeyIssued.from_dict(response.json())

        return response_201

    if response.status_code == 401:
        response_401 = ErrorEnvelope.from_dict(response.json())

        return response_401

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | KeyIssued]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: BootstrapClaimBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | KeyIssued]:
    """Exchange the deployment's single-use enrollment token for the root operator key — the bootstrap
    ceremony's one API call.

     Authenticated by the enrollment token itself (no API key exists yet). The token is minted by the
    Helm bootstrap job, consumed on first use; a consumed, expired or unknown token answers auth.invalid
    with details.reason naming which. The token is checked first; only then is name checked against the
    creation grammar (request.validation) and the provider's namespace (409 name.taken), and a refusal
    after the token check consumes no token and journals nothing (ADR-0026 d5). Journalled as issued-
    without-value in the provider chain.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (BootstrapClaimBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyIssued]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: BootstrapClaimBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | KeyIssued | None:
    """Exchange the deployment's single-use enrollment token for the root operator key — the bootstrap
    ceremony's one API call.

     Authenticated by the enrollment token itself (no API key exists yet). The token is minted by the
    Helm bootstrap job, consumed on first use; a consumed, expired or unknown token answers auth.invalid
    with details.reason naming which. The token is checked first; only then is name checked against the
    creation grammar (request.validation) and the provider's namespace (409 name.taken), and a refusal
    after the token check consumes no token and journals nothing (ADR-0026 d5). Journalled as issued-
    without-value in the provider chain.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (BootstrapClaimBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | KeyIssued
    """

    return sync_detailed(
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: BootstrapClaimBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | KeyIssued]:
    """Exchange the deployment's single-use enrollment token for the root operator key — the bootstrap
    ceremony's one API call.

     Authenticated by the enrollment token itself (no API key exists yet). The token is minted by the
    Helm bootstrap job, consumed on first use; a consumed, expired or unknown token answers auth.invalid
    with details.reason naming which. The token is checked first; only then is name checked against the
    creation grammar (request.validation) and the provider's namespace (409 name.taken), and a refusal
    after the token check consumes no token and journals nothing (ADR-0026 d5). Journalled as issued-
    without-value in the provider chain.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (BootstrapClaimBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyIssued]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: BootstrapClaimBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | KeyIssued | None:
    """Exchange the deployment's single-use enrollment token for the root operator key — the bootstrap
    ceremony's one API call.

     Authenticated by the enrollment token itself (no API key exists yet). The token is minted by the
    Helm bootstrap job, consumed on first use; a consumed, expired or unknown token answers auth.invalid
    with details.reason naming which. The token is checked first; only then is name checked against the
    creation grammar (request.validation) and the provider's namespace (409 name.taken), and a refusal
    after the token check consumes no token and journals nothing (ADR-0026 d5). Journalled as issued-
    without-value in the provider chain.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (BootstrapClaimBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | KeyIssued
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
