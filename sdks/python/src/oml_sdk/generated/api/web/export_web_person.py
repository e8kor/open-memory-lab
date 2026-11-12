from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.export_web_person_response_200 import ExportWebPersonResponse200
from ...types import UNSET, Response, Unset


def _get_kwargs(
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
        "url": "/v2/web/export",
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | ExportWebPersonResponse200:
    if response.status_code == 200:
        response_200 = ExportWebPersonResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | ExportWebPersonResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ExportWebPersonResponse200]:
    """Everything the person holds, as a JSON attachment.

     Requires: the person's web session key (<user>-web, through its capacity credential); any other key,
    the person's agent keys included, answers access.verb_ceiling with details.reason session_key_only
    (ADR-0068 d6; ADR-0070 d5). The memories of their root and below, every state, with text, and their
    keys, at most 100000 memories. Since v0.6.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ExportWebPersonResponse200]
    """

    kwargs = _get_kwargs(
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
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ExportWebPersonResponse200 | None:
    """Everything the person holds, as a JSON attachment.

     Requires: the person's web session key (<user>-web, through its capacity credential); any other key,
    the person's agent keys included, answers access.verb_ceiling with details.reason session_key_only
    (ADR-0068 d6; ADR-0070 d5). The memories of their root and below, every state, with text, and their
    keys, at most 100000 memories. Since v0.6.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ExportWebPersonResponse200
    """

    return sync_detailed(
        client=client,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ExportWebPersonResponse200]:
    """Everything the person holds, as a JSON attachment.

     Requires: the person's web session key (<user>-web, through its capacity credential); any other key,
    the person's agent keys included, answers access.verb_ceiling with details.reason session_key_only
    (ADR-0068 d6; ADR-0070 d5). The memories of their root and below, every state, with text, and their
    keys, at most 100000 memories. Since v0.6.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ExportWebPersonResponse200]
    """

    kwargs = _get_kwargs(
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ExportWebPersonResponse200 | None:
    """Everything the person holds, as a JSON attachment.

     Requires: the person's web session key (<user>-web, through its capacity credential); any other key,
    the person's agent keys included, answers access.verb_ceiling with details.reason session_key_only
    (ADR-0068 d6; ADR-0070 d5). The memories of their root and below, every state, with text, and their
    keys, at most 100000 memories. Since v0.6.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ExportWebPersonResponse200
    """

    return (
        await asyncio_detailed(
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
