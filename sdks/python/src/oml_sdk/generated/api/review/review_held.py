from http import HTTPStatus
from typing import Any
from uuid import UUID

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.held_named import HeldNamed
from ...models.held_page import HeldPage
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workarea: str | Unset = UNSET,
    author_key: str | Unset = UNSET,
    claim_key_prefix: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    ids: list[UUID] | Unset = UNSET,
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

    json_workarea: str | Unset
    if isinstance(workarea, Unset):
        json_workarea = UNSET
    else:
        json_workarea = workarea
    params["workarea"] = json_workarea

    params["author_key"] = author_key

    params["claim_key_prefix"] = claim_key_prefix

    params["limit"] = limit

    params["cursor"] = cursor

    json_ids: list[str] | Unset = UNSET
    if not isinstance(ids, Unset):
        json_ids = []
        for ids_item_data in ids:
            ids_item = str(ids_item_data)
            json_ids.append(ids_item)

    params["ids"] = json_ids

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/review/held",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | HeldNamed | HeldPage:
    if response.status_code == 200:

        def _parse_response_200(data: object) -> HeldNamed | HeldPage:
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                response_200_type_0 = HeldPage.from_dict(data)

                return response_200_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            response_200_type_1 = HeldNamed.from_dict(data)

            return response_200_type_1

        response_200 = _parse_response_200(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 429:
        response_429 = ErrorEnvelope.from_dict(response.json())

        return response_429

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | HeldNamed | HeldPage]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    author_key: str | Unset = UNSET,
    claim_key_prefix: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    ids: list[UUID] | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | HeldNamed | HeldPage]:
    """The held items this person's reviewer key may judge, with their text, oldest first; claims nothing.

     Requires: a dreamer key held_by human; any other key, a dreamer key held by an agent or a service
    included, is refused with access.verb_ceiling, details.reason bulk_review_is_human_only, before any
    lookup (ADR-0055 d1). Serves exactly what review next would serve the key: rows in its dreamer
    effective access, not claimed by another key, whose lineage closure holds neither the key nor its
    principal, and no held successor whose rival the key authored, ordered held_since then artifact_id.
    workarea narrows to that node's subtree, author_key to one authoring key by id or name,
    claim_key_prefix to claim keys that start with the text, taken literally. Claims nothing and
    journals nothing (ADR-0055 d2).

    Args:
        workarea (str | Unset): A workarea, by its node id, its name or its qualified name.
            Resolved server-side before any check that reads state; an unknown name answers exactly
            what an unknown id answers. A user key resolves a bare name in its own user root's
            namespace, plus universe and its root's name; every other key in the shared namespace
            (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is
            universe, is neither, and is refused (request.validation).
        author_key (str | Unset):
        claim_key_prefix (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        ids (list[UUID] | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | HeldNamed | HeldPage]
    """

    kwargs = _get_kwargs(
        workarea=workarea,
        author_key=author_key,
        claim_key_prefix=claim_key_prefix,
        limit=limit,
        cursor=cursor,
        ids=ids,
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
    workarea: str | Unset = UNSET,
    author_key: str | Unset = UNSET,
    claim_key_prefix: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    ids: list[UUID] | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | HeldNamed | HeldPage | None:
    """The held items this person's reviewer key may judge, with their text, oldest first; claims nothing.

     Requires: a dreamer key held_by human; any other key, a dreamer key held by an agent or a service
    included, is refused with access.verb_ceiling, details.reason bulk_review_is_human_only, before any
    lookup (ADR-0055 d1). Serves exactly what review next would serve the key: rows in its dreamer
    effective access, not claimed by another key, whose lineage closure holds neither the key nor its
    principal, and no held successor whose rival the key authored, ordered held_since then artifact_id.
    workarea narrows to that node's subtree, author_key to one authoring key by id or name,
    claim_key_prefix to claim keys that start with the text, taken literally. Claims nothing and
    journals nothing (ADR-0055 d2).

    Args:
        workarea (str | Unset): A workarea, by its node id, its name or its qualified name.
            Resolved server-side before any check that reads state; an unknown name answers exactly
            what an unknown id answers. A user key resolves a bare name in its own user root's
            namespace, plus universe and its root's name; every other key in the shared namespace
            (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is
            universe, is neither, and is refused (request.validation).
        author_key (str | Unset):
        claim_key_prefix (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        ids (list[UUID] | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | HeldNamed | HeldPage
    """

    return sync_detailed(
        client=client,
        workarea=workarea,
        author_key=author_key,
        claim_key_prefix=claim_key_prefix,
        limit=limit,
        cursor=cursor,
        ids=ids,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    author_key: str | Unset = UNSET,
    claim_key_prefix: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    ids: list[UUID] | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | HeldNamed | HeldPage]:
    """The held items this person's reviewer key may judge, with their text, oldest first; claims nothing.

     Requires: a dreamer key held_by human; any other key, a dreamer key held by an agent or a service
    included, is refused with access.verb_ceiling, details.reason bulk_review_is_human_only, before any
    lookup (ADR-0055 d1). Serves exactly what review next would serve the key: rows in its dreamer
    effective access, not claimed by another key, whose lineage closure holds neither the key nor its
    principal, and no held successor whose rival the key authored, ordered held_since then artifact_id.
    workarea narrows to that node's subtree, author_key to one authoring key by id or name,
    claim_key_prefix to claim keys that start with the text, taken literally. Claims nothing and
    journals nothing (ADR-0055 d2).

    Args:
        workarea (str | Unset): A workarea, by its node id, its name or its qualified name.
            Resolved server-side before any check that reads state; an unknown name answers exactly
            what an unknown id answers. A user key resolves a bare name in its own user root's
            namespace, plus universe and its root's name; every other key in the shared namespace
            (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is
            universe, is neither, and is refused (request.validation).
        author_key (str | Unset):
        claim_key_prefix (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        ids (list[UUID] | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | HeldNamed | HeldPage]
    """

    kwargs = _get_kwargs(
        workarea=workarea,
        author_key=author_key,
        claim_key_prefix=claim_key_prefix,
        limit=limit,
        cursor=cursor,
        ids=ids,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    author_key: str | Unset = UNSET,
    claim_key_prefix: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    ids: list[UUID] | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | HeldNamed | HeldPage | None:
    """The held items this person's reviewer key may judge, with their text, oldest first; claims nothing.

     Requires: a dreamer key held_by human; any other key, a dreamer key held by an agent or a service
    included, is refused with access.verb_ceiling, details.reason bulk_review_is_human_only, before any
    lookup (ADR-0055 d1). Serves exactly what review next would serve the key: rows in its dreamer
    effective access, not claimed by another key, whose lineage closure holds neither the key nor its
    principal, and no held successor whose rival the key authored, ordered held_since then artifact_id.
    workarea narrows to that node's subtree, author_key to one authoring key by id or name,
    claim_key_prefix to claim keys that start with the text, taken literally. Claims nothing and
    journals nothing (ADR-0055 d2).

    Args:
        workarea (str | Unset): A workarea, by its node id, its name or its qualified name.
            Resolved server-side before any check that reads state; an unknown name answers exactly
            what an unknown id answers. A user key resolves a bare name in its own user root's
            namespace, plus universe and its root's name; every other key in the shared namespace
            (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is
            universe, is neither, and is refused (request.validation).
        author_key (str | Unset):
        claim_key_prefix (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        ids (list[UUID] | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | HeldNamed | HeldPage
    """

    return (
        await asyncio_detailed(
            client=client,
            workarea=workarea,
            author_key=author_key,
            claim_key_prefix=claim_key_prefix,
            limit=limit,
            cursor=cursor,
            ids=ids,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
