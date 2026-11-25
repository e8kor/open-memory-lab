from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.import_status import ImportStatus
from ...types import UNSET, Response, Unset


def _get_kwargs(
    import_id: str,
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
        "url": "/v2/imports/{import_id}".format(
            import_id=quote(str(import_id), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | ImportStatus:
    if response.status_code == 200:
        response_200 = ImportStatus.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | ImportStatus]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    import_id: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ImportStatus]:
    """An import's state, position, counts and refusals.

     Requires: the key that ran the import, or a person's web session key, <user>-web, for an import any
    key of the person ran (ADR-0096 d6; since v0.8.5); any other key, and an id no import has, answers
    access.not_found (ADR-0084 d15). state is running, done or failed; a running import outlives no
    request, so one read running stopped when the server did, and position is the versions it recorded.
    Since v0.8.3.

    Args:
        import_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ImportStatus]
    """

    kwargs = _get_kwargs(
        import_id=import_id,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    import_id: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ImportStatus | None:
    """An import's state, position, counts and refusals.

     Requires: the key that ran the import, or a person's web session key, <user>-web, for an import any
    key of the person ran (ADR-0096 d6; since v0.8.5); any other key, and an id no import has, answers
    access.not_found (ADR-0084 d15). state is running, done or failed; a running import outlives no
    request, so one read running stopped when the server did, and position is the versions it recorded.
    Since v0.8.3.

    Args:
        import_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ImportStatus
    """

    return sync_detailed(
        import_id=import_id,
        client=client,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    import_id: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ImportStatus]:
    """An import's state, position, counts and refusals.

     Requires: the key that ran the import, or a person's web session key, <user>-web, for an import any
    key of the person ran (ADR-0096 d6; since v0.8.5); any other key, and an id no import has, answers
    access.not_found (ADR-0084 d15). state is running, done or failed; a running import outlives no
    request, so one read running stopped when the server did, and position is the versions it recorded.
    Since v0.8.3.

    Args:
        import_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ImportStatus]
    """

    kwargs = _get_kwargs(
        import_id=import_id,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    import_id: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ImportStatus | None:
    """An import's state, position, counts and refusals.

     Requires: the key that ran the import, or a person's web session key, <user>-web, for an import any
    key of the person ran (ADR-0096 d6; since v0.8.5); any other key, and an id no import has, answers
    access.not_found (ADR-0084 d15). state is running, done or failed; a running import outlives no
    request, so one read running stopped when the server did, and position is the versions it recorded.
    Since v0.8.3.

    Args:
        import_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ImportStatus
    """

    return (
        await asyncio_detailed(
            import_id=import_id,
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
