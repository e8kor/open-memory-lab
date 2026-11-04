from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.key_issue_request import KeyIssueRequest
from ...models.key_issued import KeyIssued
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: KeyIssueRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/keys",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | KeyIssued:
    if response.status_code == 201:
        response_201 = KeyIssued.from_dict(response.json())

        return response_201

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | KeyIssued]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: KeyIssueRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | KeyIssued]:
    """Issue a key under the issuer ceiling; the value appears exactly once, here.

     Requires: admin key role; operator key role only for an operator key, sent without tenant, or a
    tenant's first admin key, sent with tenant (step 0 of the issuance order). expiry and tenant are
    designed fields (openapi-designed.yaml x-oml-designed-fields), each refused with request.validation
    until it is built, so an operator key issues operator keys only (ADR-0039 d9). Journalled as issued-
    without-value (show-once ceremony). Validated in the issuance order declared in
    docs/architecture/roles-and-keys.md (Grant admissibility and refusal order), which also fixes grant
    admissibility, the default workarea, when the value is disclosed and what a part-way failure leaves
    (ADR-0025 d2). Every workarea, crew and tenant reference takes an id or a name, and the response
    carries ids under their v0.3.0 names; a v0.3.0 request spelling (default_workarea_id, tenant_id, a
    grant's workarea_node_id, a deny entry's subtree_node_id) is refused with request.validation naming
    its successor in details.renamed_to (ADR-0039 d7); name must be unique in the namespace (409
    name.taken, checked after every other check, before key.issued commits — ADR-0026 d5). The server
    takes exactly the grants it is sent. The response echoes each stored grant_id and epoch (its
    grant.issued journal seq). connect_code: true issues a connector key (ADR-0031 d7): no value is
    minted, and the 201 carries connect_code, shown once, in the value's place; an admin or operator
    role, held_by service and a deployment without oauth.issuer are refused with 422
    oauth.connect_refused and details.reason. As built the key routes keep no idempotency record, so a
    retried issuance answers name.taken and its code is minted again with oml keys connect-code.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (KeyIssueRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyIssued]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: KeyIssueRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | KeyIssued | None:
    """Issue a key under the issuer ceiling; the value appears exactly once, here.

     Requires: admin key role; operator key role only for an operator key, sent without tenant, or a
    tenant's first admin key, sent with tenant (step 0 of the issuance order). expiry and tenant are
    designed fields (openapi-designed.yaml x-oml-designed-fields), each refused with request.validation
    until it is built, so an operator key issues operator keys only (ADR-0039 d9). Journalled as issued-
    without-value (show-once ceremony). Validated in the issuance order declared in
    docs/architecture/roles-and-keys.md (Grant admissibility and refusal order), which also fixes grant
    admissibility, the default workarea, when the value is disclosed and what a part-way failure leaves
    (ADR-0025 d2). Every workarea, crew and tenant reference takes an id or a name, and the response
    carries ids under their v0.3.0 names; a v0.3.0 request spelling (default_workarea_id, tenant_id, a
    grant's workarea_node_id, a deny entry's subtree_node_id) is refused with request.validation naming
    its successor in details.renamed_to (ADR-0039 d7); name must be unique in the namespace (409
    name.taken, checked after every other check, before key.issued commits — ADR-0026 d5). The server
    takes exactly the grants it is sent. The response echoes each stored grant_id and epoch (its
    grant.issued journal seq). connect_code: true issues a connector key (ADR-0031 d7): no value is
    minted, and the 201 carries connect_code, shown once, in the value's place; an admin or operator
    role, held_by service and a deployment without oauth.issuer are refused with 422
    oauth.connect_refused and details.reason. As built the key routes keep no idempotency record, so a
    retried issuance answers name.taken and its code is minted again with oml keys connect-code.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (KeyIssueRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | KeyIssued
    """

    return sync_detailed(
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: KeyIssueRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | KeyIssued]:
    """Issue a key under the issuer ceiling; the value appears exactly once, here.

     Requires: admin key role; operator key role only for an operator key, sent without tenant, or a
    tenant's first admin key, sent with tenant (step 0 of the issuance order). expiry and tenant are
    designed fields (openapi-designed.yaml x-oml-designed-fields), each refused with request.validation
    until it is built, so an operator key issues operator keys only (ADR-0039 d9). Journalled as issued-
    without-value (show-once ceremony). Validated in the issuance order declared in
    docs/architecture/roles-and-keys.md (Grant admissibility and refusal order), which also fixes grant
    admissibility, the default workarea, when the value is disclosed and what a part-way failure leaves
    (ADR-0025 d2). Every workarea, crew and tenant reference takes an id or a name, and the response
    carries ids under their v0.3.0 names; a v0.3.0 request spelling (default_workarea_id, tenant_id, a
    grant's workarea_node_id, a deny entry's subtree_node_id) is refused with request.validation naming
    its successor in details.renamed_to (ADR-0039 d7); name must be unique in the namespace (409
    name.taken, checked after every other check, before key.issued commits — ADR-0026 d5). The server
    takes exactly the grants it is sent. The response echoes each stored grant_id and epoch (its
    grant.issued journal seq). connect_code: true issues a connector key (ADR-0031 d7): no value is
    minted, and the 201 carries connect_code, shown once, in the value's place; an admin or operator
    role, held_by service and a deployment without oauth.issuer are refused with 422
    oauth.connect_refused and details.reason. As built the key routes keep no idempotency record, so a
    retried issuance answers name.taken and its code is minted again with oml keys connect-code.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (KeyIssueRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyIssued]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: KeyIssueRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | KeyIssued | None:
    """Issue a key under the issuer ceiling; the value appears exactly once, here.

     Requires: admin key role; operator key role only for an operator key, sent without tenant, or a
    tenant's first admin key, sent with tenant (step 0 of the issuance order). expiry and tenant are
    designed fields (openapi-designed.yaml x-oml-designed-fields), each refused with request.validation
    until it is built, so an operator key issues operator keys only (ADR-0039 d9). Journalled as issued-
    without-value (show-once ceremony). Validated in the issuance order declared in
    docs/architecture/roles-and-keys.md (Grant admissibility and refusal order), which also fixes grant
    admissibility, the default workarea, when the value is disclosed and what a part-way failure leaves
    (ADR-0025 d2). Every workarea, crew and tenant reference takes an id or a name, and the response
    carries ids under their v0.3.0 names; a v0.3.0 request spelling (default_workarea_id, tenant_id, a
    grant's workarea_node_id, a deny entry's subtree_node_id) is refused with request.validation naming
    its successor in details.renamed_to (ADR-0039 d7); name must be unique in the namespace (409
    name.taken, checked after every other check, before key.issued commits — ADR-0026 d5). The server
    takes exactly the grants it is sent. The response echoes each stored grant_id and epoch (its
    grant.issued journal seq). connect_code: true issues a connector key (ADR-0031 d7): no value is
    minted, and the 201 carries connect_code, shown once, in the value's place; an admin or operator
    role, held_by service and a deployment without oauth.issuer are refused with 422
    oauth.connect_refused and details.reason. As built the key routes keep no idempotency record, so a
    retried issuance answers name.taken and its code is minted again with oml keys connect-code.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (KeyIssueRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | KeyIssued
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            idempotency_key=idempotency_key,
        )
    ).parsed
