from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.workarea_counts import WorkareaCounts
from ...models.workarea_page import WorkareaPage
from ...types import UNSET, Response, Unset


def _get_kwargs(
    workarea: str,
    *,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    counts: bool | Unset = UNSET,
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

    params["limit"] = limit

    params["cursor"] = cursor

    params["counts"] = counts

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/workareas/{workarea}/tree".format(
            workarea=quote(str(workarea), safe=""),
        ),
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | WorkareaCounts | WorkareaPage:
    if response.status_code == 200:

        def _parse_response_200(data: object) -> WorkareaCounts | WorkareaPage:
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                response_200_type_0 = WorkareaPage.from_dict(data)

                return response_200_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            response_200_type_1 = WorkareaCounts.from_dict(data)

            return response_200_type_1

        response_200 = _parse_response_200(response.json())

        return response_200

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | WorkareaCounts | WorkareaPage]:
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
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    counts: bool | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | WorkareaCounts | WorkareaPage]:
    """The subtree, paginated; nodes outside read scope are omitted.

     Requires: reader effective access, or the admin key role, which reads the structure of every
    workarea in its tenant without grants and never claim content (docs/architecture/roles-and-keys.md,
    permission matrix footnote 6; ADR-0026 d15).

    Args:
        workarea (str): A workarea, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        counts (bool | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WorkareaCounts | WorkareaPage]
    """

    kwargs = _get_kwargs(
        workarea=workarea,
        limit=limit,
        cursor=cursor,
        counts=counts,
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
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    counts: bool | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | WorkareaCounts | WorkareaPage | None:
    """The subtree, paginated; nodes outside read scope are omitted.

     Requires: reader effective access, or the admin key role, which reads the structure of every
    workarea in its tenant without grants and never claim content (docs/architecture/roles-and-keys.md,
    permission matrix footnote 6; ADR-0026 d15).

    Args:
        workarea (str): A workarea, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        counts (bool | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WorkareaCounts | WorkareaPage
    """

    return sync_detailed(
        workarea=workarea,
        client=client,
        limit=limit,
        cursor=cursor,
        counts=counts,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    workarea: str,
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    counts: bool | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | WorkareaCounts | WorkareaPage]:
    """The subtree, paginated; nodes outside read scope are omitted.

     Requires: reader effective access, or the admin key role, which reads the structure of every
    workarea in its tenant without grants and never claim content (docs/architecture/roles-and-keys.md,
    permission matrix footnote 6; ADR-0026 d15).

    Args:
        workarea (str): A workarea, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        counts (bool | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WorkareaCounts | WorkareaPage]
    """

    kwargs = _get_kwargs(
        workarea=workarea,
        limit=limit,
        cursor=cursor,
        counts=counts,
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
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    counts: bool | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | WorkareaCounts | WorkareaPage | None:
    """The subtree, paginated; nodes outside read scope are omitted.

     Requires: reader effective access, or the admin key role, which reads the structure of every
    workarea in its tenant without grants and never claim content (docs/architecture/roles-and-keys.md,
    permission matrix footnote 6; ADR-0026 d15).

    Args:
        workarea (str): A workarea, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        counts (bool | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WorkareaCounts | WorkareaPage
    """

    return (
        await asyncio_detailed(
            workarea=workarea,
            client=client,
            limit=limit,
            cursor=cursor,
            counts=counts,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
