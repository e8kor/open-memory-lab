from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    upload_id: str,
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
        "method": "delete",
        "url": "/v2/uploads/{upload_id}".format(
            upload_id=quote(str(upload_id), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | ErrorEnvelope:
    if response.status_code == 204:
        response_204 = cast(Any, None)
        return response_204

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    upload_id: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[Any | ErrorEnvelope]:
    """Abandon an upload session, shredding its key and removing its parts.

     Requires: the key that opened the session, or access.not_found (ADR-0004 d3, as amended 2026-11-17).
    An open, completed or failed session is abandoned: its key is nulled, then its parts are removed.
    One already abandoned or expired answers the same; a bound one belongs to the verb that bound it and
    is upload.session_invalid. Nothing is journalled. Since v0.8.3.

    Args:
        upload_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        upload_id=upload_id,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    upload_id: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Any | ErrorEnvelope | None:
    """Abandon an upload session, shredding its key and removing its parts.

     Requires: the key that opened the session, or access.not_found (ADR-0004 d3, as amended 2026-11-17).
    An open, completed or failed session is abandoned: its key is nulled, then its parts are removed.
    One already abandoned or expired answers the same; a bound one belongs to the verb that bound it and
    is upload.session_invalid. Nothing is journalled. Since v0.8.3.

    Args:
        upload_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ErrorEnvelope
    """

    return sync_detailed(
        upload_id=upload_id,
        client=client,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    upload_id: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[Any | ErrorEnvelope]:
    """Abandon an upload session, shredding its key and removing its parts.

     Requires: the key that opened the session, or access.not_found (ADR-0004 d3, as amended 2026-11-17).
    An open, completed or failed session is abandoned: its key is nulled, then its parts are removed.
    One already abandoned or expired answers the same; a bound one belongs to the verb that bound it and
    is upload.session_invalid. Nothing is journalled. Since v0.8.3.

    Args:
        upload_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        upload_id=upload_id,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    upload_id: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Any | ErrorEnvelope | None:
    """Abandon an upload session, shredding its key and removing its parts.

     Requires: the key that opened the session, or access.not_found (ADR-0004 d3, as amended 2026-11-17).
    An open, completed or failed session is abandoned: its key is nulled, then its parts are removed.
    One already abandoned or expired answers the same; a bound one belongs to the verb that bound it and
    is upload.session_invalid. Nothing is journalled. Since v0.8.3.

    Args:
        upload_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            upload_id=upload_id,
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
