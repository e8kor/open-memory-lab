"""The wrapper's seven verbs send only what the caller set and refuse an
argument outside the mode its fields choose before sending anything
(ADR-0039 d3, d4, d10; docs/engineering/sdks.md)."""

import json

import httpx
import pytest

from oml_sdk import ApiError, OmlClient, RequestError

ACCEPTED = {
    "artifact_id": "art_1",
    "claim_key": "auto-0123456789abcdef",
    "version": 1,
    "state": "held",
    "plaintext_digest": "sha256:" + "0" * 64,
    "journal_seq": 1,
    "workarea_id": "wa_1",
    "retirement": False,
    "replaces": None,
}


def recording(status: int, answer: dict | None):
    """A transport that records each request and answers `status` with `answer`."""
    requests: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        requests.append(request)
        return httpx.Response(status, json=answer) if answer is not None else httpx.Response(status)

    return requests, httpx.MockTransport(handler)


def client(transport: httpx.MockTransport) -> OmlClient:
    return OmlClient(url="http://oml.test", api_key="oml_test_key", transport=transport)


def test_write_without_claim_key_sends_none():
    requests, transport = recording(202, ACCEPTED)
    with client(transport) as oml:
        written = oml.write("A claim with no key of its own.")
    assert [(r.method, r.url.path) for r in requests] == [("POST", "/v1/memories")]
    assert json.loads(requests[0].content) == {"body": "A claim with no key of its own."}
    assert requests[0].headers["idempotency-key"] == written.idempotency_key
    assert written.value.claim_key == "auto-0123456789abcdef"


def test_write_with_replaces_supersedes_its_target_and_sends_the_reason():
    requests, transport = recording(202, ACCEPTED | {"version": 2})
    with client(transport) as oml:
        oml.write("Retired: the runbook moved.", replaces="art_1", retirement=True, reason="moved")
    assert [(r.method, r.url.path) for r in requests] == [("POST", "/v1/memories/art_1/supersede")]
    assert json.loads(requests[0].content) == {
        "body": "Retired: the runbook moved.",
        "retirement": True,
        "reason": "moved",
    }


@pytest.mark.parametrize(
    ("arguments", "field"),
    [
        ({"retirement": True}, "/retirement"),
        ({"reason": "why"}, "/reason"),
        ({"replaces": "art_1", "claim_key": "k"}, "/claim_key"),
    ],
)
def test_a_write_argument_outside_its_mode_raises_before_sending(arguments, field):
    requests, transport = recording(500, None)
    with client(transport) as oml, pytest.raises(RequestError) as caught:
        oml.write("Some text.", **arguments)
    assert (caught.value.code, caught.value.details) == ("request.validation", {"field": field})
    assert requests == []


def test_read_with_artifact_id_and_query_raises_before_sending():
    requests, transport = recording(500, None)
    with client(transport) as oml, pytest.raises(RequestError) as caught:
        oml.read("art_1", query="canary stage")
    assert caught.value.code == "request.validation"
    assert caught.value.details == {"field": "/query"}
    assert requests == []


@pytest.mark.parametrize(
    ("arguments", "field"),
    [
        ({"artifact_id": "art_1", "workarea": "w"}, "/workarea"),
        ({"query": "q", "own": True}, "/own"),
        ({"k": 3}, "/k"),
    ],
)
def test_a_read_argument_outside_its_mode_raises_before_sending(arguments, field):
    requests, transport = recording(500, None)
    with client(transport) as oml, pytest.raises(RequestError) as caught:
        oml.read(**arguments)
    assert caught.value.details == {"field": field}
    assert requests == []


def test_read_with_neither_lists_the_keys_own_rows():
    requests, transport = recording(200, {"items": [], "next_cursor": None})
    with client(transport) as oml:
        page = oml.read(limit=5)
    assert page.items == []
    assert [(r.method, r.url.path, dict(r.url.params)) for r in requests] == [
        ("GET", "/v1/memories", {"own": "true", "limit": "5"})
    ]


def test_approve_without_serve_id_sends_none():
    result = {"artifact_id": "art_1", "version": 1, "verdict": "approve", "state": "promoted"}
    requests, transport = recording(200, result)
    with client(transport) as oml:
        written = oml.approve()
    assert json.loads(requests[0].content) == {"verdict": "approve"}
    assert written.value.state == "promoted"


def test_reject_sends_its_reason_and_serve_id():
    result = {"artifact_id": "art_1", "version": 1, "verdict": "reject", "state": "rejected"}
    requests, transport = recording(200, result)
    with client(transport) as oml:
        oml.reject("srv_1", reason="no source")
    assert json.loads(requests[0].content) == {
        "serve_id": "srv_1",
        "verdict": "reject",
        "reason": "no source",
    }


def test_a_refusal_raises_the_envelopes_code_and_details():
    envelope = {
        "error": {
            "code": "request.validation",
            "message": "query is refused with artifact_id",
            "rule": "The contract is openapi.yaml; what it does not admit is refused, not coerced.",
            "details": {"field": "/query"},
            "retryable": False,
        },
        "request_id": "req_7",
    }
    _, transport = recording(400, envelope)
    with client(transport) as oml, pytest.raises(ApiError) as caught:
        oml.read(query="canary stage")
    error = caught.value
    assert (error.status, error.code, error.details) == (
        400,
        "request.validation",
        {"field": "/query"},
    )
    assert (error.message, error.rule, error.request_id) == (
        envelope["error"]["message"],
        envelope["error"]["rule"],
        "req_7",
    )
    assert error.retryable is False


def test_workarea_without_a_name_reads_the_keys_default_workarea(oml_server, agent):
    view = agent.workarea()
    assert view.id == oml_server.workarea
    assert "remember" in view.my_effective_verbs
    assert agent.workarea(oml_server.review_workarea).id == oml_server.review_workarea


def test_events_tails_the_named_workarea_from_after_seq(oml_server, agent, marker):
    # The key's own workarea, never the review queue's, which the agent loop reads whole.
    written = agent.write(f"An event to tail, {marker}.", workarea=oml_server.workarea)
    page = agent.events(workarea=oml_server.workarea)
    assert page.head_seq >= written.value.journal_seq
    later = agent.events(workarea=oml_server.workarea, after_seq=page.head_seq)
    assert later.items == []
