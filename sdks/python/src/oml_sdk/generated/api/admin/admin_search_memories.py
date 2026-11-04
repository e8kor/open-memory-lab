from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.admin_search_memories_body import AdminSearchMemoriesBody
from ...models.admin_search_memories_response_200 import AdminSearchMemoriesResponse200
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: AdminSearchMemoriesBody,
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
        "url": "/v2/admin/memories/search",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> AdminSearchMemoriesResponse200 | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = AdminSearchMemoriesResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[AdminSearchMemoriesResponse200 | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: AdminSearchMemoriesBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[AdminSearchMemoriesResponse200 | ErrorEnvelope]:
    """Search every memory of the tenant, in every state, workarea and user root, with its text and
    attribution.

     Requires: admin key role (ADR-0045 d21); the operator and every other role answer
    access.verb_ceiling. query is the recall full-text search without the visibility filter; state and
    workarea narrow it. A POST so that the query travels in a body, never a path or query string. Each
    answer journals one memory.admin_read event naming the admin key, the artifact_ids served and a
    query_digest, never the text or the query. Served on the lead only. Records no verdict, creates no
    serve binding. Since v0.5.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AdminSearchMemoriesBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AdminSearchMemoriesResponse200 | ErrorEnvelope]
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
    body: AdminSearchMemoriesBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> AdminSearchMemoriesResponse200 | ErrorEnvelope | None:
    """Search every memory of the tenant, in every state, workarea and user root, with its text and
    attribution.

     Requires: admin key role (ADR-0045 d21); the operator and every other role answer
    access.verb_ceiling. query is the recall full-text search without the visibility filter; state and
    workarea narrow it. A POST so that the query travels in a body, never a path or query string. Each
    answer journals one memory.admin_read event naming the admin key, the artifact_ids served and a
    query_digest, never the text or the query. Served on the lead only. Records no verdict, creates no
    serve binding. Since v0.5.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AdminSearchMemoriesBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AdminSearchMemoriesResponse200 | ErrorEnvelope
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
    body: AdminSearchMemoriesBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[AdminSearchMemoriesResponse200 | ErrorEnvelope]:
    """Search every memory of the tenant, in every state, workarea and user root, with its text and
    attribution.

     Requires: admin key role (ADR-0045 d21); the operator and every other role answer
    access.verb_ceiling. query is the recall full-text search without the visibility filter; state and
    workarea narrow it. A POST so that the query travels in a body, never a path or query string. Each
    answer journals one memory.admin_read event naming the admin key, the artifact_ids served and a
    query_digest, never the text or the query. Served on the lead only. Records no verdict, creates no
    serve binding. Since v0.5.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AdminSearchMemoriesBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AdminSearchMemoriesResponse200 | ErrorEnvelope]
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
    body: AdminSearchMemoriesBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> AdminSearchMemoriesResponse200 | ErrorEnvelope | None:
    """Search every memory of the tenant, in every state, workarea and user root, with its text and
    attribution.

     Requires: admin key role (ADR-0045 d21); the operator and every other role answer
    access.verb_ceiling. query is the recall full-text search without the visibility filter; state and
    workarea narrow it. A POST so that the query travels in a body, never a path or query string. Each
    answer journals one memory.admin_read event naming the admin key, the artifact_ids served and a
    query_digest, never the text or the query. Served on the lead only. Records no verdict, creates no
    serve binding. Since v0.5.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AdminSearchMemoriesBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AdminSearchMemoriesResponse200 | ErrorEnvelope
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
