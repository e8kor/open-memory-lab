from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.submit_accepted import SubmitAccepted
from ...models.supersede_request import SupersedeRequest
from ...types import UNSET, Response, Unset


def _get_kwargs(
    artifact_id: str,
    *,
    body: SupersedeRequest,
    idempotency_key: str,
    x_oml_workarea: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    if not isinstance(x_oml_workarea, Unset):
        headers["X-OML-Workarea"] = x_oml_workarea

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/memories/{artifact_id}/supersede".format(
            artifact_id=quote(str(artifact_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | SubmitAccepted:
    if response.status_code == 202:
        response_202 = SubmitAccepted.from_dict(response.json())

        return response_202

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | SubmitAccepted]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: SupersedeRequest,
    idempotency_key: str,
    x_oml_workarea: str | Unset = UNSET,
) -> Response[ErrorEnvelope | SubmitAccepted]:
    """Propose a replacement version, or with retirement true propose retiring the claim; a new attributed
    submission, held like any write.

     Requires: the supersede rule declared in docs/architecture/roles-and-keys.md (Grant admissibility
    and refusal order) — a writer on a target version it authored, a dreamer through edit-as-proposal.
    The target must be the current served tip: submission.tip_conflict, with details.current_tip when
    the caller can read it. The tip moves only at ratification, by compare-and-set; the tip, version-
    ordinal, competing-successor, retirement and retired-claim rules are declared in
    docs/architecture/audit-and-lineage.md (ADR-0025 d10, d11). X-OML-Workarea, if sent, must equal the
    target's workarea (request.validation otherwise); that check, the tip, the kind and retirement
    checks and workarea.frozen are answered only after the target is known readable, in the order of
    step 5 of the supersede rule. Idempotency-Key is scoped to the key with a nil workarea; the record
    is looked up after step 1 of that rule and before step 2, so a committed supersession replays its
    stored response whatever the target's state or the key's access now, and it is claimed only after
    step 2 passes. Omitted attachments and terms leave the successor with none; only kind is inherited.
    A tainted replacement can never take the default tip (simulation.tip_refused at promotion): it
    shadows the tip for its simulation audience only.

    Args:
        artifact_id (str):
        idempotency_key (str):
        x_oml_workarea (str | Unset):
        body (SupersedeRequest): Unknown fields are refused (request.validation). The body
            declares every submission field a successor can need — simulation_id, simulated_time,
            mapping and the term fields — so closing it refuses nothing a supersession must carry
            (ADR-0025 d16).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SubmitAccepted]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
        body=body,
        idempotency_key=idempotency_key,
        x_oml_workarea=x_oml_workarea,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: SupersedeRequest,
    idempotency_key: str,
    x_oml_workarea: str | Unset = UNSET,
) -> ErrorEnvelope | SubmitAccepted | None:
    """Propose a replacement version, or with retirement true propose retiring the claim; a new attributed
    submission, held like any write.

     Requires: the supersede rule declared in docs/architecture/roles-and-keys.md (Grant admissibility
    and refusal order) — a writer on a target version it authored, a dreamer through edit-as-proposal.
    The target must be the current served tip: submission.tip_conflict, with details.current_tip when
    the caller can read it. The tip moves only at ratification, by compare-and-set; the tip, version-
    ordinal, competing-successor, retirement and retired-claim rules are declared in
    docs/architecture/audit-and-lineage.md (ADR-0025 d10, d11). X-OML-Workarea, if sent, must equal the
    target's workarea (request.validation otherwise); that check, the tip, the kind and retirement
    checks and workarea.frozen are answered only after the target is known readable, in the order of
    step 5 of the supersede rule. Idempotency-Key is scoped to the key with a nil workarea; the record
    is looked up after step 1 of that rule and before step 2, so a committed supersession replays its
    stored response whatever the target's state or the key's access now, and it is claimed only after
    step 2 passes. Omitted attachments and terms leave the successor with none; only kind is inherited.
    A tainted replacement can never take the default tip (simulation.tip_refused at promotion): it
    shadows the tip for its simulation audience only.

    Args:
        artifact_id (str):
        idempotency_key (str):
        x_oml_workarea (str | Unset):
        body (SupersedeRequest): Unknown fields are refused (request.validation). The body
            declares every submission field a successor can need — simulation_id, simulated_time,
            mapping and the term fields — so closing it refuses nothing a supersession must carry
            (ADR-0025 d16).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SubmitAccepted
    """

    return sync_detailed(
        artifact_id=artifact_id,
        client=client,
        body=body,
        idempotency_key=idempotency_key,
        x_oml_workarea=x_oml_workarea,
    ).parsed


async def asyncio_detailed(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: SupersedeRequest,
    idempotency_key: str,
    x_oml_workarea: str | Unset = UNSET,
) -> Response[ErrorEnvelope | SubmitAccepted]:
    """Propose a replacement version, or with retirement true propose retiring the claim; a new attributed
    submission, held like any write.

     Requires: the supersede rule declared in docs/architecture/roles-and-keys.md (Grant admissibility
    and refusal order) — a writer on a target version it authored, a dreamer through edit-as-proposal.
    The target must be the current served tip: submission.tip_conflict, with details.current_tip when
    the caller can read it. The tip moves only at ratification, by compare-and-set; the tip, version-
    ordinal, competing-successor, retirement and retired-claim rules are declared in
    docs/architecture/audit-and-lineage.md (ADR-0025 d10, d11). X-OML-Workarea, if sent, must equal the
    target's workarea (request.validation otherwise); that check, the tip, the kind and retirement
    checks and workarea.frozen are answered only after the target is known readable, in the order of
    step 5 of the supersede rule. Idempotency-Key is scoped to the key with a nil workarea; the record
    is looked up after step 1 of that rule and before step 2, so a committed supersession replays its
    stored response whatever the target's state or the key's access now, and it is claimed only after
    step 2 passes. Omitted attachments and terms leave the successor with none; only kind is inherited.
    A tainted replacement can never take the default tip (simulation.tip_refused at promotion): it
    shadows the tip for its simulation audience only.

    Args:
        artifact_id (str):
        idempotency_key (str):
        x_oml_workarea (str | Unset):
        body (SupersedeRequest): Unknown fields are refused (request.validation). The body
            declares every submission field a successor can need — simulation_id, simulated_time,
            mapping and the term fields — so closing it refuses nothing a supersession must carry
            (ADR-0025 d16).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SubmitAccepted]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
        body=body,
        idempotency_key=idempotency_key,
        x_oml_workarea=x_oml_workarea,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: SupersedeRequest,
    idempotency_key: str,
    x_oml_workarea: str | Unset = UNSET,
) -> ErrorEnvelope | SubmitAccepted | None:
    """Propose a replacement version, or with retirement true propose retiring the claim; a new attributed
    submission, held like any write.

     Requires: the supersede rule declared in docs/architecture/roles-and-keys.md (Grant admissibility
    and refusal order) — a writer on a target version it authored, a dreamer through edit-as-proposal.
    The target must be the current served tip: submission.tip_conflict, with details.current_tip when
    the caller can read it. The tip moves only at ratification, by compare-and-set; the tip, version-
    ordinal, competing-successor, retirement and retired-claim rules are declared in
    docs/architecture/audit-and-lineage.md (ADR-0025 d10, d11). X-OML-Workarea, if sent, must equal the
    target's workarea (request.validation otherwise); that check, the tip, the kind and retirement
    checks and workarea.frozen are answered only after the target is known readable, in the order of
    step 5 of the supersede rule. Idempotency-Key is scoped to the key with a nil workarea; the record
    is looked up after step 1 of that rule and before step 2, so a committed supersession replays its
    stored response whatever the target's state or the key's access now, and it is claimed only after
    step 2 passes. Omitted attachments and terms leave the successor with none; only kind is inherited.
    A tainted replacement can never take the default tip (simulation.tip_refused at promotion): it
    shadows the tip for its simulation audience only.

    Args:
        artifact_id (str):
        idempotency_key (str):
        x_oml_workarea (str | Unset):
        body (SupersedeRequest): Unknown fields are refused (request.validation). The body
            declares every submission field a successor can need — simulation_id, simulated_time,
            mapping and the term fields — so closing it refuses nothing a supersession must carry
            (ADR-0025 d16).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SubmitAccepted
    """

    return (
        await asyncio_detailed(
            artifact_id=artifact_id,
            client=client,
            body=body,
            idempotency_key=idempotency_key,
            x_oml_workarea=x_oml_workarea,
        )
    ).parsed
