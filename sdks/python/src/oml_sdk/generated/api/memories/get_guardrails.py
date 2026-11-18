from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.guardrail_set import GuardrailSet
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workspace: str | Unset = UNSET,
    include_own_held: bool | Unset = False,
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

    params["include_own_held"] = include_own_held

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
    workspace: str | Unset = UNSET,
    include_own_held: bool | Unset = False,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | GuardrailSet]:
    """The guardrails in force at the scope node, under the key's reader access and guardrail scope; never
    in recall.

     Requires: reader on the scope node. Serves the promoted guardrail tips on the node and, under
    guardrail_scope ancestors, on each ancestor the key reads; the scope is the key's, set at issuance,
    and any parameter other than workspace and include_own_held, guardrail_scope included, is
    request.validation naming it. On a node inside a workspace a person put in progress, every author's
    held guardrail is served too, and under include_own_held the key's own held ones: each a line of its
    own after the promoted rows, never a held retirement or a version the scan held, marked unreviewed
    with effect_inside ask, so a held rule asks and never denies (docs/api/http-api.md, Guardrails;
    ADR-0037 d4, d5, d7; ADR-0080 d10, d14, d16; since v0.5.1).

    Args:
        workspace (str | Unset):
        include_own_held (bool | Unset):  Default: False.
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
        workspace=workspace,
        include_own_held=include_own_held,
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
    include_own_held: bool | Unset = False,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | GuardrailSet | None:
    """The guardrails in force at the scope node, under the key's reader access and guardrail scope; never
    in recall.

     Requires: reader on the scope node. Serves the promoted guardrail tips on the node and, under
    guardrail_scope ancestors, on each ancestor the key reads; the scope is the key's, set at issuance,
    and any parameter other than workspace and include_own_held, guardrail_scope included, is
    request.validation naming it. On a node inside a workspace a person put in progress, every author's
    held guardrail is served too, and under include_own_held the key's own held ones: each a line of its
    own after the promoted rows, never a held retirement or a version the scan held, marked unreviewed
    with effect_inside ask, so a held rule asks and never denies (docs/api/http-api.md, Guardrails;
    ADR-0037 d4, d5, d7; ADR-0080 d10, d14, d16; since v0.5.1).

    Args:
        workspace (str | Unset):
        include_own_held (bool | Unset):  Default: False.
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
        workspace=workspace,
        include_own_held=include_own_held,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    include_own_held: bool | Unset = False,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | GuardrailSet]:
    """The guardrails in force at the scope node, under the key's reader access and guardrail scope; never
    in recall.

     Requires: reader on the scope node. Serves the promoted guardrail tips on the node and, under
    guardrail_scope ancestors, on each ancestor the key reads; the scope is the key's, set at issuance,
    and any parameter other than workspace and include_own_held, guardrail_scope included, is
    request.validation naming it. On a node inside a workspace a person put in progress, every author's
    held guardrail is served too, and under include_own_held the key's own held ones: each a line of its
    own after the promoted rows, never a held retirement or a version the scan held, marked unreviewed
    with effect_inside ask, so a held rule asks and never denies (docs/api/http-api.md, Guardrails;
    ADR-0037 d4, d5, d7; ADR-0080 d10, d14, d16; since v0.5.1).

    Args:
        workspace (str | Unset):
        include_own_held (bool | Unset):  Default: False.
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
        workspace=workspace,
        include_own_held=include_own_held,
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
    include_own_held: bool | Unset = False,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | GuardrailSet | None:
    """The guardrails in force at the scope node, under the key's reader access and guardrail scope; never
    in recall.

     Requires: reader on the scope node. Serves the promoted guardrail tips on the node and, under
    guardrail_scope ancestors, on each ancestor the key reads; the scope is the key's, set at issuance,
    and any parameter other than workspace and include_own_held, guardrail_scope included, is
    request.validation naming it. On a node inside a workspace a person put in progress, every author's
    held guardrail is served too, and under include_own_held the key's own held ones: each a line of its
    own after the promoted rows, never a held retirement or a version the scan held, marked unreviewed
    with effect_inside ask, so a held rule asks and never denies (docs/api/http-api.md, Guardrails;
    ADR-0037 d4, d5, d7; ADR-0080 d10, d14, d16; since v0.5.1).

    Args:
        workspace (str | Unset):
        include_own_held (bool | Unset):  Default: False.
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
            workspace=workspace,
            include_own_held=include_own_held,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
