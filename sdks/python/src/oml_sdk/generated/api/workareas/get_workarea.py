from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
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
        "url": "/v2/workareas/{workarea}".format(
            workarea=quote(str(workarea), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope:
    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope]:
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
) -> Response[ErrorEnvelope]:
    """One node — id, parent, derived path, status, crew record — with the calling key's view of it.

     Requires: reader effective access (access.not_found otherwise), or the admin key role, which reads
    the structure of every workarea in its tenant without grants and never claim content — for an admin
    key every child and edge is present and my_effective_verbs is empty (docs/architecture/roles-and-
    keys.md, permission matrix footnote 6; ADR-0026 d15). children, my_effective_verbs and
    permission_epoch are relative to the calling key: children (ordered by path_leaf, then id) and edges
    (ordered by edge_id) outside read access are absent; my_effective_verbs is the union over the grant
    roles effective on the node (the effective-verbs table in docs/architecture/roles-and-keys.md);
    permission_epoch is the calling key's permission epoch as declared in docs/architecture/workareas.md
    (ADR-0025 d14). One call carries all of it.

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
        Response[ErrorEnvelope]
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
) -> ErrorEnvelope | None:
    """One node — id, parent, derived path, status, crew record — with the calling key's view of it.

     Requires: reader effective access (access.not_found otherwise), or the admin key role, which reads
    the structure of every workarea in its tenant without grants and never claim content — for an admin
    key every child and edge is present and my_effective_verbs is empty (docs/architecture/roles-and-
    keys.md, permission matrix footnote 6; ADR-0026 d15). children, my_effective_verbs and
    permission_epoch are relative to the calling key: children (ordered by path_leaf, then id) and edges
    (ordered by edge_id) outside read access are absent; my_effective_verbs is the union over the grant
    roles effective on the node (the effective-verbs table in docs/architecture/roles-and-keys.md);
    permission_epoch is the calling key's permission epoch as declared in docs/architecture/workareas.md
    (ADR-0025 d14). One call carries all of it.

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
        ErrorEnvelope
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
) -> Response[ErrorEnvelope]:
    """One node — id, parent, derived path, status, crew record — with the calling key's view of it.

     Requires: reader effective access (access.not_found otherwise), or the admin key role, which reads
    the structure of every workarea in its tenant without grants and never claim content — for an admin
    key every child and edge is present and my_effective_verbs is empty (docs/architecture/roles-and-
    keys.md, permission matrix footnote 6; ADR-0026 d15). children, my_effective_verbs and
    permission_epoch are relative to the calling key: children (ordered by path_leaf, then id) and edges
    (ordered by edge_id) outside read access are absent; my_effective_verbs is the union over the grant
    roles effective on the node (the effective-verbs table in docs/architecture/roles-and-keys.md);
    permission_epoch is the calling key's permission epoch as declared in docs/architecture/workareas.md
    (ADR-0025 d14). One call carries all of it.

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
        Response[ErrorEnvelope]
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
) -> ErrorEnvelope | None:
    """One node — id, parent, derived path, status, crew record — with the calling key's view of it.

     Requires: reader effective access (access.not_found otherwise), or the admin key role, which reads
    the structure of every workarea in its tenant without grants and never claim content — for an admin
    key every child and edge is present and my_effective_verbs is empty (docs/architecture/roles-and-
    keys.md, permission matrix footnote 6; ADR-0026 d15). children, my_effective_verbs and
    permission_epoch are relative to the calling key: children (ordered by path_leaf, then id) and edges
    (ordered by edge_id) outside read access are absent; my_effective_verbs is the union over the grant
    roles effective on the node (the effective-verbs table in docs/architecture/roles-and-keys.md);
    permission_epoch is the calling key's permission epoch as declared in docs/architecture/workareas.md
    (ADR-0025 d14). One call carries all of it.

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
        ErrorEnvelope
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
