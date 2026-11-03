from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.vocab_suggest_page import VocabSuggestPage
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    q: str,
    workarea_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["q"] = q

    json_workarea_id: str | Unset
    if isinstance(workarea_id, Unset):
        json_workarea_id = UNSET
    else:
        json_workarea_id = workarea_id
    params["workarea_id"] = json_workarea_id

    params["limit"] = limit

    params["cursor"] = cursor

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/vocab/suggest",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | VocabSuggestPage:
    if response.status_code == 200:
        response_200 = VocabSuggestPage.from_dict(response.json())

        return response_200

    if response.status_code == 429:
        response_429 = ErrorEnvelope.from_dict(response.json())

        return response_429

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | VocabSuggestPage]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    q: str,
    workarea_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> Response[ErrorEnvelope | VocabSuggestPage]:
    """Deterministic prefix+label typeahead over granted vocabulary workareas — matching, never
    interpretation.

     Requires: reader effective access on the vocabulary workarea(s) searched; vocabularies outside scope
    are absent (fail closed).

    Args:
        q (str):
        workarea_id (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | VocabSuggestPage]
    """

    kwargs = _get_kwargs(
        q=q,
        workarea_id=workarea_id,
        limit=limit,
        cursor=cursor,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    q: str,
    workarea_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> ErrorEnvelope | VocabSuggestPage | None:
    """Deterministic prefix+label typeahead over granted vocabulary workareas — matching, never
    interpretation.

     Requires: reader effective access on the vocabulary workarea(s) searched; vocabularies outside scope
    are absent (fail closed).

    Args:
        q (str):
        workarea_id (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | VocabSuggestPage
    """

    return sync_detailed(
        client=client,
        q=q,
        workarea_id=workarea_id,
        limit=limit,
        cursor=cursor,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    q: str,
    workarea_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> Response[ErrorEnvelope | VocabSuggestPage]:
    """Deterministic prefix+label typeahead over granted vocabulary workareas — matching, never
    interpretation.

     Requires: reader effective access on the vocabulary workarea(s) searched; vocabularies outside scope
    are absent (fail closed).

    Args:
        q (str):
        workarea_id (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | VocabSuggestPage]
    """

    kwargs = _get_kwargs(
        q=q,
        workarea_id=workarea_id,
        limit=limit,
        cursor=cursor,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    q: str,
    workarea_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> ErrorEnvelope | VocabSuggestPage | None:
    """Deterministic prefix+label typeahead over granted vocabulary workareas — matching, never
    interpretation.

     Requires: reader effective access on the vocabulary workarea(s) searched; vocabularies outside scope
    are absent (fail closed).

    Args:
        q (str):
        workarea_id (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | VocabSuggestPage
    """

    return (
        await asyncio_detailed(
            client=client,
            q=q,
            workarea_id=workarea_id,
            limit=limit,
            cursor=cursor,
        )
    ).parsed
