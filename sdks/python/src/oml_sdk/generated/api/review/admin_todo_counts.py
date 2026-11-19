from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.admin_todo_counts_response_200 import AdminTodoCountsResponse200
from ...models.error_envelope import ErrorEnvelope
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
        "url": "/v2/admin/todo/counts",
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> AdminTodoCountsResponse200 | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = AdminTodoCountsResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[AdminTodoCountsResponse200 | ErrorEnvelope]:
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
) -> Response[AdminTodoCountsResponse200 | ErrorEnvelope]:
    """The administration To do page's counts; no content.

     Requires: a person's admin session key, <user>-admin, through its capacity credential; any other
    key, <user>-curate and a tenant admin key included, answers access.verb_ceiling with details.reason
    session_key_only (ADR-0083 d11). Its groups (unconfirmed admin fixes, guardrail-loosening alerts,
    open investigations) join with ADR-0089's acts; until then it answers truncated alone, false
    (ADR-0083 d14). Takes no query parameter. Journals nothing. Since v0.8.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AdminTodoCountsResponse200 | ErrorEnvelope]
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
) -> AdminTodoCountsResponse200 | ErrorEnvelope | None:
    """The administration To do page's counts; no content.

     Requires: a person's admin session key, <user>-admin, through its capacity credential; any other
    key, <user>-curate and a tenant admin key included, answers access.verb_ceiling with details.reason
    session_key_only (ADR-0083 d11). Its groups (unconfirmed admin fixes, guardrail-loosening alerts,
    open investigations) join with ADR-0089's acts; until then it answers truncated alone, false
    (ADR-0083 d14). Takes no query parameter. Journals nothing. Since v0.8.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AdminTodoCountsResponse200 | ErrorEnvelope
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
) -> Response[AdminTodoCountsResponse200 | ErrorEnvelope]:
    """The administration To do page's counts; no content.

     Requires: a person's admin session key, <user>-admin, through its capacity credential; any other
    key, <user>-curate and a tenant admin key included, answers access.verb_ceiling with details.reason
    session_key_only (ADR-0083 d11). Its groups (unconfirmed admin fixes, guardrail-loosening alerts,
    open investigations) join with ADR-0089's acts; until then it answers truncated alone, false
    (ADR-0083 d14). Takes no query parameter. Journals nothing. Since v0.8.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AdminTodoCountsResponse200 | ErrorEnvelope]
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
) -> AdminTodoCountsResponse200 | ErrorEnvelope | None:
    """The administration To do page's counts; no content.

     Requires: a person's admin session key, <user>-admin, through its capacity credential; any other
    key, <user>-curate and a tenant admin key included, answers access.verb_ceiling with details.reason
    session_key_only (ADR-0083 d11). Its groups (unconfirmed admin fixes, guardrail-loosening alerts,
    open investigations) join with ADR-0089's acts; until then it answers truncated alone, false
    (ADR-0083 d14). Takes no query parameter. Journals nothing. Since v0.8.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AdminTodoCountsResponse200 | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
