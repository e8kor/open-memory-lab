from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.admin_act import AdminAct
from ...models.error_envelope import ErrorEnvelope
from ...models.federation_settings import FederationSettings
from ...models.set_federation_policy_body import SetFederationPolicyBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: SetFederationPolicyBody,
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
        "url": "/v2/federation",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> AdminAct | ErrorEnvelope | FederationSettings:
    if response.status_code == 200:
        response_200 = FederationSettings.from_dict(response.json())

        return response_200

    if response.status_code == 202:
        response_202 = AdminAct.from_dict(response.json())

        return response_202

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[AdminAct | ErrorEnvelope | FederationSettings]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: SetFederationPolicyBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[AdminAct | ErrorEnvelope | FederationSettings]:
    """Set the tenant's federation policy; each field sent replaces its value.

     Requires: admin key role (ADR-0072 d5). max_sources above 64 is request.validation at /max_sources,
    and a default_sources other than scope or wide at /default_sources. Journalled as
    federation.policy_set with the policy, the previous one and the admin key; a change that changes
    nothing journals nothing. The change is an admin act (ADR-0097 d3, d5): one that searches less
    (enabled false, default_sources scope, fewer max_sources) is narrowing and takes effect at once; one
    that searches more is widening, at once marked sole_admin in a tenant that has only ever had one
    admin person, and otherwise answers 202 with the pending act, requires a reason, and applies over
    the policy as it stands at its confirmation; one that does both is request.validation at /, two acts
    of two classes. Since v0.7.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SetFederationPolicyBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AdminAct | ErrorEnvelope | FederationSettings]
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
    body: SetFederationPolicyBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> AdminAct | ErrorEnvelope | FederationSettings | None:
    """Set the tenant's federation policy; each field sent replaces its value.

     Requires: admin key role (ADR-0072 d5). max_sources above 64 is request.validation at /max_sources,
    and a default_sources other than scope or wide at /default_sources. Journalled as
    federation.policy_set with the policy, the previous one and the admin key; a change that changes
    nothing journals nothing. The change is an admin act (ADR-0097 d3, d5): one that searches less
    (enabled false, default_sources scope, fewer max_sources) is narrowing and takes effect at once; one
    that searches more is widening, at once marked sole_admin in a tenant that has only ever had one
    admin person, and otherwise answers 202 with the pending act, requires a reason, and applies over
    the policy as it stands at its confirmation; one that does both is request.validation at /, two acts
    of two classes. Since v0.7.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SetFederationPolicyBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AdminAct | ErrorEnvelope | FederationSettings
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
    body: SetFederationPolicyBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[AdminAct | ErrorEnvelope | FederationSettings]:
    """Set the tenant's federation policy; each field sent replaces its value.

     Requires: admin key role (ADR-0072 d5). max_sources above 64 is request.validation at /max_sources,
    and a default_sources other than scope or wide at /default_sources. Journalled as
    federation.policy_set with the policy, the previous one and the admin key; a change that changes
    nothing journals nothing. The change is an admin act (ADR-0097 d3, d5): one that searches less
    (enabled false, default_sources scope, fewer max_sources) is narrowing and takes effect at once; one
    that searches more is widening, at once marked sole_admin in a tenant that has only ever had one
    admin person, and otherwise answers 202 with the pending act, requires a reason, and applies over
    the policy as it stands at its confirmation; one that does both is request.validation at /, two acts
    of two classes. Since v0.7.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SetFederationPolicyBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AdminAct | ErrorEnvelope | FederationSettings]
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
    body: SetFederationPolicyBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> AdminAct | ErrorEnvelope | FederationSettings | None:
    """Set the tenant's federation policy; each field sent replaces its value.

     Requires: admin key role (ADR-0072 d5). max_sources above 64 is request.validation at /max_sources,
    and a default_sources other than scope or wide at /default_sources. Journalled as
    federation.policy_set with the policy, the previous one and the admin key; a change that changes
    nothing journals nothing. The change is an admin act (ADR-0097 d3, d5): one that searches less
    (enabled false, default_sources scope, fewer max_sources) is narrowing and takes effect at once; one
    that searches more is widening, at once marked sole_admin in a tenant that has only ever had one
    admin person, and otherwise answers 202 with the pending act, requires a reason, and applies over
    the policy as it stands at its confirmation; one that does both is request.validation at /, two acts
    of two classes. Since v0.7.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SetFederationPolicyBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AdminAct | ErrorEnvelope | FederationSettings
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
