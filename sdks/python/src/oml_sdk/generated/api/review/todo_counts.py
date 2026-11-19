from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.todo_counts_response_200 import TodoCountsResponse200
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workspace: str | Unset = UNSET,
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

    params["workspace"] = workspace

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/todo/counts",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | TodoCountsResponse200:
    if response.status_code == 200:
        response_200 = TodoCountsResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | TodoCountsResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | TodoCountsResponse200]:
    """The To do page's counts for a person's web session key; no content.

     Requires: a person's web session key, <user>-web, through its capacity credential; any other key
    answers access.verb_ceiling with details.reason session_key_only (ADR-0083 d11). review is the
    number of rows GET /v2/review/held would serve the person's reviewer key, <user>-review, at
    workspace and below it, or across every workspace without workspace, computed under the same grants,
    lineage exclusions and author fences as that listing; counted up to 1,000, with truncated true past
    it, and read up to 2,000 held rows, counted or not, with truncated true and the count so far once
    that bound stops the read (ADR-0083 d16). A person with no live reviewer key or no reviewer grant
    counts 0, and so does a workspace that key cannot read or that does not exist, which is never named
    or refused. No memory id, text, claim key or name. Journals nothing. The other groups (fixes,
    ratify, withdrawals, overrides, imports) join with their features (ADR-0083 d14). Since v0.8.3.

    Args:
        workspace (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | TodoCountsResponse200]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
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
    workspace: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | TodoCountsResponse200 | None:
    """The To do page's counts for a person's web session key; no content.

     Requires: a person's web session key, <user>-web, through its capacity credential; any other key
    answers access.verb_ceiling with details.reason session_key_only (ADR-0083 d11). review is the
    number of rows GET /v2/review/held would serve the person's reviewer key, <user>-review, at
    workspace and below it, or across every workspace without workspace, computed under the same grants,
    lineage exclusions and author fences as that listing; counted up to 1,000, with truncated true past
    it, and read up to 2,000 held rows, counted or not, with truncated true and the count so far once
    that bound stops the read (ADR-0083 d16). A person with no live reviewer key or no reviewer grant
    counts 0, and so does a workspace that key cannot read or that does not exist, which is never named
    or refused. No memory id, text, claim key or name. Journals nothing. The other groups (fixes,
    ratify, withdrawals, overrides, imports) join with their features (ADR-0083 d14). Since v0.8.3.

    Args:
        workspace (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | TodoCountsResponse200
    """

    return sync_detailed(
        client=client,
        workspace=workspace,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | TodoCountsResponse200]:
    """The To do page's counts for a person's web session key; no content.

     Requires: a person's web session key, <user>-web, through its capacity credential; any other key
    answers access.verb_ceiling with details.reason session_key_only (ADR-0083 d11). review is the
    number of rows GET /v2/review/held would serve the person's reviewer key, <user>-review, at
    workspace and below it, or across every workspace without workspace, computed under the same grants,
    lineage exclusions and author fences as that listing; counted up to 1,000, with truncated true past
    it, and read up to 2,000 held rows, counted or not, with truncated true and the count so far once
    that bound stops the read (ADR-0083 d16). A person with no live reviewer key or no reviewer grant
    counts 0, and so does a workspace that key cannot read or that does not exist, which is never named
    or refused. No memory id, text, claim key or name. Journals nothing. The other groups (fixes,
    ratify, withdrawals, overrides, imports) join with their features (ADR-0083 d14). Since v0.8.3.

    Args:
        workspace (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | TodoCountsResponse200]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | TodoCountsResponse200 | None:
    """The To do page's counts for a person's web session key; no content.

     Requires: a person's web session key, <user>-web, through its capacity credential; any other key
    answers access.verb_ceiling with details.reason session_key_only (ADR-0083 d11). review is the
    number of rows GET /v2/review/held would serve the person's reviewer key, <user>-review, at
    workspace and below it, or across every workspace without workspace, computed under the same grants,
    lineage exclusions and author fences as that listing; counted up to 1,000, with truncated true past
    it, and read up to 2,000 held rows, counted or not, with truncated true and the count so far once
    that bound stops the read (ADR-0083 d16). A person with no live reviewer key or no reviewer grant
    counts 0, and so does a workspace that key cannot read or that does not exist, which is never named
    or refused. No memory id, text, claim key or name. Journals nothing. The other groups (fixes,
    ratify, withdrawals, overrides, imports) join with their features (ADR-0083 d14). Since v0.8.3.

    Args:
        workspace (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | TodoCountsResponse200
    """

    return (
        await asyncio_detailed(
            client=client,
            workspace=workspace,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
