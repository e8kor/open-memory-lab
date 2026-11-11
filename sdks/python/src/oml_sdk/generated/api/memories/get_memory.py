import datetime
from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.memory import Memory
from ...types import UNSET, Response, Unset


def _get_kwargs(
    artifact_id: str,
    *,
    as_of: datetime.datetime | Unset = UNSET,
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

    params: dict[str, Any] = {}

    json_as_of: str | Unset = UNSET
    if not isinstance(as_of, Unset):
        json_as_of = as_of.isoformat()
    params["as_of"] = json_as_of

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/memories/{artifact_id}".format(
            artifact_id=quote(str(artifact_id), safe=""),
        ),
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | Memory:
    if response.status_code == 200:
        response_200 = Memory.from_dict(response.json())

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
) -> Response[ErrorEnvelope | Memory]:
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
    as_of: datetime.datetime | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | Memory]:
    """One memory with its text (decrypted on the serving path) and attachment descriptors.

     The calling key may fetch its own rows in any state, with no grant needed (global denials still
    mask). Anyone else needs reader effective access on the row's workarea and sees promoted versions
    only — superseded versions and every version of a retired claim stay fetchable as history. Get takes
    the row's own workarea as its implicit scope, so a non-own self_signed version is served to a key
    with reader effective access there. Anything else answers access.not_found (ADR-0025 d3). Erased
    answers 410 lifecycle.erased only to a key that could read the version immediately before its
    erasure — its author's key set, or, when it was promoted then, a key with reader effective access on
    its workarea; readability is checked first, and every other key gets access.not_found. Resolves no
    workarea: an X-OML-Workarea header is request.validation, details.field X-OML-Workarea. derives_from
    lists only ancestors the caller can read, replaces is null when the caller cannot read the replaced
    version, and workarea_id is null on an own row whose node the key holds no role on. With as_of, a
    version recorded after the instant is access.not_found and any other answers as get does today
    (ADR-0059 d19); as_of is the only query parameter get takes.

    Args:
        artifact_id (str):
        as_of (datetime.datetime | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Memory]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
        as_of=as_of,
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
    as_of: datetime.datetime | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | Memory | None:
    """One memory with its text (decrypted on the serving path) and attachment descriptors.

     The calling key may fetch its own rows in any state, with no grant needed (global denials still
    mask). Anyone else needs reader effective access on the row's workarea and sees promoted versions
    only — superseded versions and every version of a retired claim stay fetchable as history. Get takes
    the row's own workarea as its implicit scope, so a non-own self_signed version is served to a key
    with reader effective access there. Anything else answers access.not_found (ADR-0025 d3). Erased
    answers 410 lifecycle.erased only to a key that could read the version immediately before its
    erasure — its author's key set, or, when it was promoted then, a key with reader effective access on
    its workarea; readability is checked first, and every other key gets access.not_found. Resolves no
    workarea: an X-OML-Workarea header is request.validation, details.field X-OML-Workarea. derives_from
    lists only ancestors the caller can read, replaces is null when the caller cannot read the replaced
    version, and workarea_id is null on an own row whose node the key holds no role on. With as_of, a
    version recorded after the instant is access.not_found and any other answers as get does today
    (ADR-0059 d19); as_of is the only query parameter get takes.

    Args:
        artifact_id (str):
        as_of (datetime.datetime | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Memory
    """

    return sync_detailed(
        artifact_id=artifact_id,
        client=client,
        as_of=as_of,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    as_of: datetime.datetime | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | Memory]:
    """One memory with its text (decrypted on the serving path) and attachment descriptors.

     The calling key may fetch its own rows in any state, with no grant needed (global denials still
    mask). Anyone else needs reader effective access on the row's workarea and sees promoted versions
    only — superseded versions and every version of a retired claim stay fetchable as history. Get takes
    the row's own workarea as its implicit scope, so a non-own self_signed version is served to a key
    with reader effective access there. Anything else answers access.not_found (ADR-0025 d3). Erased
    answers 410 lifecycle.erased only to a key that could read the version immediately before its
    erasure — its author's key set, or, when it was promoted then, a key with reader effective access on
    its workarea; readability is checked first, and every other key gets access.not_found. Resolves no
    workarea: an X-OML-Workarea header is request.validation, details.field X-OML-Workarea. derives_from
    lists only ancestors the caller can read, replaces is null when the caller cannot read the replaced
    version, and workarea_id is null on an own row whose node the key holds no role on. With as_of, a
    version recorded after the instant is access.not_found and any other answers as get does today
    (ADR-0059 d19); as_of is the only query parameter get takes.

    Args:
        artifact_id (str):
        as_of (datetime.datetime | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Memory]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
        as_of=as_of,
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
    as_of: datetime.datetime | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | Memory | None:
    """One memory with its text (decrypted on the serving path) and attachment descriptors.

     The calling key may fetch its own rows in any state, with no grant needed (global denials still
    mask). Anyone else needs reader effective access on the row's workarea and sees promoted versions
    only — superseded versions and every version of a retired claim stay fetchable as history. Get takes
    the row's own workarea as its implicit scope, so a non-own self_signed version is served to a key
    with reader effective access there. Anything else answers access.not_found (ADR-0025 d3). Erased
    answers 410 lifecycle.erased only to a key that could read the version immediately before its
    erasure — its author's key set, or, when it was promoted then, a key with reader effective access on
    its workarea; readability is checked first, and every other key gets access.not_found. Resolves no
    workarea: an X-OML-Workarea header is request.validation, details.field X-OML-Workarea. derives_from
    lists only ancestors the caller can read, replaces is null when the caller cannot read the replaced
    version, and workarea_id is null on an own row whose node the key holds no role on. With as_of, a
    version recorded after the instant is access.not_found and any other answers as get does today
    (ADR-0059 d19); as_of is the only query parameter get takes.

    Args:
        artifact_id (str):
        as_of (datetime.datetime | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Memory
    """

    return (
        await asyncio_detailed(
            artifact_id=artifact_id,
            client=client,
            as_of=as_of,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
