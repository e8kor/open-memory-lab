from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.guardrail_set import GuardrailSet
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workarea: str | Unset = UNSET,
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

    params["workarea"] = workarea

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/guardrails",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | GuardrailSet:
    if response.status_code == 200:
        response_200 = GuardrailSet.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = ErrorEnvelope.from_dict(response.json())

        return response_401

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | GuardrailSet]:
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
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | GuardrailSet]:
    """The guardrails in force at the scope node, under the key's reader access and guardrail scope; never
    in recall.

     Requires: reader on the scope node. Serves the promoted guardrail tips on the node and, under
    guardrail_scope ancestors, on each ancestor the key reads; the scope is the key's, set at issuance,
    and any parameter other than workarea, guardrail_scope included, is request.validation naming it
    (docs/api/http-api.md, Guardrails; ADR-0037 d4, d5, d7; since v0.5.1).

    Args:
        workarea (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | GuardrailSet]
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
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | GuardrailSet | None:
    """The guardrails in force at the scope node, under the key's reader access and guardrail scope; never
    in recall.

     Requires: reader on the scope node. Serves the promoted guardrail tips on the node and, under
    guardrail_scope ancestors, on each ancestor the key reads; the scope is the key's, set at issuance,
    and any parameter other than workarea, guardrail_scope included, is request.validation naming it
    (docs/api/http-api.md, Guardrails; ADR-0037 d4, d5, d7; since v0.5.1).

    Args:
        workarea (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | GuardrailSet
    """

    return sync_detailed(
        client=client,
        workarea=workarea,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | GuardrailSet]:
    """The guardrails in force at the scope node, under the key's reader access and guardrail scope; never
    in recall.

     Requires: reader on the scope node. Serves the promoted guardrail tips on the node and, under
    guardrail_scope ancestors, on each ancestor the key reads; the scope is the key's, set at issuance,
    and any parameter other than workarea, guardrail_scope included, is request.validation naming it
    (docs/api/http-api.md, Guardrails; ADR-0037 d4, d5, d7; since v0.5.1).

    Args:
        workarea (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | GuardrailSet]
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
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | GuardrailSet | None:
    """The guardrails in force at the scope node, under the key's reader access and guardrail scope; never
    in recall.

     Requires: reader on the scope node. Serves the promoted guardrail tips on the node and, under
    guardrail_scope ancestors, on each ancestor the key reads; the scope is the key's, set at issuance,
    and any parameter other than workarea, guardrail_scope included, is request.validation naming it
    (docs/api/http-api.md, Guardrails; ADR-0037 d4, d5, d7; since v0.5.1).

    Args:
        workarea (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | GuardrailSet
    """

    return (
        await asyncio_detailed(
            client=client,
            workarea=workarea,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
