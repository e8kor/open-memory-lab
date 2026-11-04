from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.admin_read_memory_response_200 import AdminReadMemoryResponse200
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    artifact_id: str,
    *,
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
        "method": "get",
        "url": "/v2/admin/memories/{artifact_id}".format(
            artifact_id=quote(str(artifact_id), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> AdminReadMemoryResponse200 | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = AdminReadMemoryResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[AdminReadMemoryResponse200 | ErrorEnvelope]:
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
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[AdminReadMemoryResponse200 | ErrorEnvelope]:
    """One memory of the tenant with its text and full attribution, a universe claim with its source.

     Requires: admin key role (ADR-0045 d21). The fields of the search plus derives_from, replaces, every
    verdict with its reviewing key, principal, reason and time, and plaintext_digest; ADR-0044 d9's
    served view does not apply. Journalled as memory.admin_read with the one artifact_id. Served on the
    lead only. Since v0.5.0.

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AdminReadMemoryResponse200 | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> AdminReadMemoryResponse200 | ErrorEnvelope | None:
    """One memory of the tenant with its text and full attribution, a universe claim with its source.

     Requires: admin key role (ADR-0045 d21). The fields of the search plus derives_from, replaces, every
    verdict with its reviewing key, principal, reason and time, and plaintext_digest; ADR-0044 d9's
    served view does not apply. Journalled as memory.admin_read with the one artifact_id. Served on the
    lead only. Since v0.5.0.

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AdminReadMemoryResponse200 | ErrorEnvelope
    """

    return sync_detailed(
        artifact_id=artifact_id,
        client=client,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[AdminReadMemoryResponse200 | ErrorEnvelope]:
    """One memory of the tenant with its text and full attribution, a universe claim with its source.

     Requires: admin key role (ADR-0045 d21). The fields of the search plus derives_from, replaces, every
    verdict with its reviewing key, principal, reason and time, and plaintext_digest; ADR-0044 d9's
    served view does not apply. Journalled as memory.admin_read with the one artifact_id. Served on the
    lead only. Since v0.5.0.

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AdminReadMemoryResponse200 | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> AdminReadMemoryResponse200 | ErrorEnvelope | None:
    """One memory of the tenant with its text and full attribution, a universe claim with its source.

     Requires: admin key role (ADR-0045 d21). The fields of the search plus derives_from, replaces, every
    verdict with its reviewing key, principal, reason and time, and plaintext_digest; ADR-0044 d9's
    served view does not apply. Journalled as memory.admin_read with the one artifact_id. Served on the
    lead only. Since v0.5.0.

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AdminReadMemoryResponse200 | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            artifact_id=artifact_id,
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
