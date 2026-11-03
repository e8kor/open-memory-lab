"""One Idempotency-Key per write, stable across that call's retries (docs/engineering/sdks.md)."""

import json
import secrets

import httpx
import pytest

from oml_sdk import IdempotencyError, OmlClient


class LoseFirstSubmitResponse(httpx.BaseTransport):
    """Forwards every request to the real server, and loses the first submit's
    response after the server has answered it: the lost-response retry."""

    def __init__(self) -> None:
        self._inner = httpx.HTTPTransport()
        self.lost = 0
        self.keys: list[str | None] = []

    def handle_request(self, request: httpx.Request) -> httpx.Response:
        response = self._inner.handle_request(request)
        if request.method == "POST" and request.url.path == "/v1/memories":
            self.keys.append(request.headers.get("idempotency-key"))
            if self.lost == 0:
                self.lost += 1
                response.read()
                response.close()
                raise httpx.ReadError("simulated: the response was lost", request=request)
        return response

    def close(self) -> None:
        self._inner.close()


class CountSubmits(httpx.BaseTransport):
    def __init__(self) -> None:
        self._inner = httpx.HTTPTransport()
        self.submits = 0

    def handle_request(self, request: httpx.Request) -> httpx.Response:
        if request.method == "POST" and request.url.path == "/v1/memories":
            self.submits += 1
        return self._inner.handle_request(request)

    def close(self) -> None:
        self._inner.close()


def test_a_write_whose_response_was_lost_is_retried_under_the_same_key_and_replayed(
    oml_server, agent
):
    claim_key = f"lost-{secrets.token_hex(4)}"
    transport = LoseFirstSubmitResponse()
    with OmlClient(
        base_url=oml_server.url, api_key=oml_server.agent_key, transport=transport
    ) as client:
        written = client.remember(claim_key, "A write whose first response never arrived.")

    assert transport.lost == 1
    assert transport.keys == [written.idempotency_key, written.idempotency_key]
    assert written.replayed is True
    assert written.value.state == "held"
    rows = list(agent.iter_memories(own=True, claim_key=claim_key))
    assert [row.artifact_id for row in rows] == [written.value.artifact_id]


def test_a_caller_supplied_key_replays_the_first_write(agent):
    claim_key = f"replay-{secrets.token_hex(4)}"
    key = f"sdk-test/{secrets.token_hex(8)}"
    first = agent.remember(claim_key, "Written once, sent twice.", idempotency_key=key)
    second = agent.remember(claim_key, "Written once, sent twice.", idempotency_key=key)
    assert (first.replayed, second.replayed) == (False, True)
    assert first.idempotency_key == second.idempotency_key == key
    assert second.value.artifact_id == first.value.artifact_id


def test_a_key_reused_with_a_different_body_is_refused_and_not_retried(oml_server, agent):
    claim_key = f"mismatch-{secrets.token_hex(4)}"
    key = f"sdk-test/{secrets.token_hex(8)}"
    agent.remember(claim_key, "The first body.", idempotency_key=key)
    transport = CountSubmits()
    with (
        OmlClient(
            base_url=oml_server.url, api_key=oml_server.agent_key, transport=transport
        ) as client,
        pytest.raises(IdempotencyError) as caught,
    ):
        client.remember(claim_key, "A different body.", idempotency_key=key)
    assert caught.value.code == "idempotency.digest_mismatch"
    assert caught.value.status == 409
    assert caught.value.retryable is False
    assert transport.submits == 1


def test_each_call_sends_a_fresh_key(agent):
    tag = secrets.token_hex(4)
    one = agent.remember(f"fresh-{tag}-1", "The first of two writes.")
    two = agent.remember(f"fresh-{tag}-2", "The second of two writes.")
    assert one.idempotency_key != two.idempotency_key
    assert (one.replayed, two.replayed) == (False, False)


def test_a_retryable_refusal_is_retried_under_the_same_key():
    sent: list[str | None] = []
    accepted = {
        "artifact_id": "art_1",
        "claim_key": "retry",
        "version": 1,
        "state": "held",
        "plaintext_digest": "sha256:" + "0" * 64,
        "journal_seq": 1,
        "workarea_id": "wa_1",
        "retirement": False,
        "replaces": None,
    }

    def handler(request: httpx.Request) -> httpx.Response:
        sent.append(request.headers.get("idempotency-key"))
        if len(sent) == 1:
            envelope = {
                "error": {
                    "code": "system.unavailable",
                    "message": "the fork check has not verified",
                    "rule": "A replica that cannot prove its journal head accepts no writes (D1).",
                    "retryable": True,
                },
                "request_id": "req_1",
            }
            return httpx.Response(503, headers={"Retry-After": "0"}, content=json.dumps(envelope))
        return httpx.Response(202, json=accepted)

    with OmlClient(
        base_url="http://oml.test", api_key="oml_test_key", transport=httpx.MockTransport(handler)
    ) as client:
        written = client.remember("retry", "Retried once.")
    assert len(sent) == 2 and sent[0] == sent[1] == written.idempotency_key
    assert written.value.artifact_id == "art_1"
