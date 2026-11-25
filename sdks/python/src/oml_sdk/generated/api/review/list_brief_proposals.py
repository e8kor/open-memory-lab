from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.list_brief_proposals_response_200 import ListBriefProposalsResponse200
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workspace: str | Unset = UNSET,
    cursor: str | Unset = UNSET,
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

    params["cursor"] = cursor

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/review/briefs",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | ListBriefProposalsResponse200:
    if response.status_code == 200:
        response_200 = ListBriefProposalsResponse200.from_dict(response.json())

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
) -> Response[ErrorEnvelope | ListBriefProposalsResponse200]:
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
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ListBriefProposalsResponse200]:
    """The pending brief edges a person's reviewer key may ratify, oldest first.

     Requires: a person's review session key, <user>-review, through its capacity credential; any other
    caller is access.verb_ceiling (ADR-0096 d7). The pending brief edges on workspaces where the key
    holds reviewer effective access, at workspace and below where one is named, oldest first, a page of
    50; an edge proposed by the key itself or under its principal is not listed, since ratification
    refuses it (review.ratifier_is_proposer). same_user is true where the proposer acts for the
    reviewer's person (ADR-0046 d2). One read of the pending edges whatever the number of workspaces.
    Journals nothing. Since v0.8.5.

    Args:
        workspace (str | Unset):
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ListBriefProposalsResponse200]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        cursor=cursor,
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
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ListBriefProposalsResponse200 | None:
    """The pending brief edges a person's reviewer key may ratify, oldest first.

     Requires: a person's review session key, <user>-review, through its capacity credential; any other
    caller is access.verb_ceiling (ADR-0096 d7). The pending brief edges on workspaces where the key
    holds reviewer effective access, at workspace and below where one is named, oldest first, a page of
    50; an edge proposed by the key itself or under its principal is not listed, since ratification
    refuses it (review.ratifier_is_proposer). same_user is true where the proposer acts for the
    reviewer's person (ADR-0046 d2). One read of the pending edges whatever the number of workspaces.
    Journals nothing. Since v0.8.5.

    Args:
        workspace (str | Unset):
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ListBriefProposalsResponse200
    """

    return sync_detailed(
        client=client,
        workspace=workspace,
        cursor=cursor,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ListBriefProposalsResponse200]:
    """The pending brief edges a person's reviewer key may ratify, oldest first.

     Requires: a person's review session key, <user>-review, through its capacity credential; any other
    caller is access.verb_ceiling (ADR-0096 d7). The pending brief edges on workspaces where the key
    holds reviewer effective access, at workspace and below where one is named, oldest first, a page of
    50; an edge proposed by the key itself or under its principal is not listed, since ratification
    refuses it (review.ratifier_is_proposer). same_user is true where the proposer acts for the
    reviewer's person (ADR-0046 d2). One read of the pending edges whatever the number of workspaces.
    Journals nothing. Since v0.8.5.

    Args:
        workspace (str | Unset):
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ListBriefProposalsResponse200]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        cursor=cursor,
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
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ListBriefProposalsResponse200 | None:
    """The pending brief edges a person's reviewer key may ratify, oldest first.

     Requires: a person's review session key, <user>-review, through its capacity credential; any other
    caller is access.verb_ceiling (ADR-0096 d7). The pending brief edges on workspaces where the key
    holds reviewer effective access, at workspace and below where one is named, oldest first, a page of
    50; an edge proposed by the key itself or under its principal is not listed, since ratification
    refuses it (review.ratifier_is_proposer). same_user is true where the proposer acts for the
    reviewer's person (ADR-0046 d2). One read of the pending edges whatever the number of workspaces.
    Journals nothing. Since v0.8.5.

    Args:
        workspace (str | Unset):
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ListBriefProposalsResponse200
    """

    return (
        await asyncio_detailed(
            client=client,
            workspace=workspace,
            cursor=cursor,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
