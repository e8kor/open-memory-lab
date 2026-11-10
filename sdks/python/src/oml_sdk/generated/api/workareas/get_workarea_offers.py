from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.workarea_offers import WorkareaOffers
from ...types import UNSET, Response, Unset


def _get_kwargs(
    workarea: str,
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
        "url": "/v2/workareas/{workarea}/offers".format(
            workarea=quote(str(workarea), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | WorkareaOffers:
    if response.status_code == 200:
        response_200 = WorkareaOffers.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | WorkareaOffers]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    workarea: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | WorkareaOffers]:
    """The transitions, the mode change and the commit the caller's verbs admit on a workarea now.

     Requires: reader effective access on the workarea, or the admin key role; one the caller cannot read
    is access.not_found (R3). Each act is offered only where the verb's own check admits the caller now:
    propose the next rung, ratify a live proposal another key made, put the workarea in progress, or
    commit an in-progress root; a commit the held rows refuse is not offered and names their count.
    Journals nothing (ADR-0045 d28; ADR-0070 d5). Since v0.6.4.

    Args:
        workarea (str): A workarea, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WorkareaOffers]
    """

    kwargs = _get_kwargs(
        workarea=workarea,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    workarea: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | WorkareaOffers | None:
    """The transitions, the mode change and the commit the caller's verbs admit on a workarea now.

     Requires: reader effective access on the workarea, or the admin key role; one the caller cannot read
    is access.not_found (R3). Each act is offered only where the verb's own check admits the caller now:
    propose the next rung, ratify a live proposal another key made, put the workarea in progress, or
    commit an in-progress root; a commit the held rows refuse is not offered and names their count.
    Journals nothing (ADR-0045 d28; ADR-0070 d5). Since v0.6.4.

    Args:
        workarea (str): A workarea, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WorkareaOffers
    """

    return sync_detailed(
        workarea=workarea,
        client=client,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    workarea: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | WorkareaOffers]:
    """The transitions, the mode change and the commit the caller's verbs admit on a workarea now.

     Requires: reader effective access on the workarea, or the admin key role; one the caller cannot read
    is access.not_found (R3). Each act is offered only where the verb's own check admits the caller now:
    propose the next rung, ratify a live proposal another key made, put the workarea in progress, or
    commit an in-progress root; a commit the held rows refuse is not offered and names their count.
    Journals nothing (ADR-0045 d28; ADR-0070 d5). Since v0.6.4.

    Args:
        workarea (str): A workarea, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WorkareaOffers]
    """

    kwargs = _get_kwargs(
        workarea=workarea,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workarea: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | WorkareaOffers | None:
    """The transitions, the mode change and the commit the caller's verbs admit on a workarea now.

     Requires: reader effective access on the workarea, or the admin key role; one the caller cannot read
    is access.not_found (R3). Each act is offered only where the verb's own check admits the caller now:
    propose the next rung, ratify a live proposal another key made, put the workarea in progress, or
    commit an in-progress root; a commit the held rows refuse is not offered and names their count.
    Journals nothing (ADR-0045 d28; ADR-0070 d5). Since v0.6.4.

    Args:
        workarea (str): A workarea, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WorkareaOffers
    """

    return (
        await asyncio_detailed(
            workarea=workarea,
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
