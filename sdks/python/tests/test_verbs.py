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
    "workspace_id": "wa_1",
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
        oml.write(
            "Retired: the runbook moved.",
            replaces="art_1",
            expected_version=1,
            retirement=True,
            reason="moved",
        )
    assert [(r.method, r.url.path) for r in requests] == [("POST", "/v1/memories/art_1/supersede")]
    assert json.loads(requests[0].content) == {
        "body": "Retired: the runbook moved.",
        "expected_version": 1,
        "retirement": True,
        "reason": "moved",
    }


@pytest.mark.parametrize(
    ("arguments", "field"),
    [
        ({"retirement": True}, "/retirement"),
        ({"reason": "why"}, "/reason"),
        ({"replaces": "art_1", "claim_key": "k"}, "/claim_key"),
        ({"replaces": "art_1"}, "/expected_version"),
        ({"expected_version": 1}, "/expected_version"),
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
        ({"artifact_id": "art_1", "workspace": "w"}, "/workspace"),
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


def test_read_with_neither_reads_the_catalog_and_own_true_the_keys_own_rows_with_text():
    requests, transport = recording(200, {"items": [], "next_cursor": None})
    with client(transport) as oml:
        page = oml.read(limit=5)
        oml.read(own=True, kinds=["skill"], prefix="skill/")
    assert page.items == []
    assert [(r.method, r.url.path, dict(r.url.params)) for r in requests] == [
        ("GET", "/v1/memories", {"own": "false", "limit": "5"}),
        (
            "GET",
            "/v1/memories",
            {"own": "true", "kinds": "skill", "prefix": "skill/", "include_text": "true"},
        ),
    ]


def test_the_listing_iterators_read_the_catalog_unless_the_caller_passes_own_true():
    requests, transport = recording(200, {"items": [], "next_cursor": None})
    with client(transport) as oml:
        list(oml.iter_memories(limit=5))
        list(oml.pages(own=True, limit=5))
    assert [dict(r.url.params) for r in requests] == [
        {"own": "false", "limit": "5"},
        {"own": "true", "include_text": "true", "limit": "5"},
    ]


def test_write_with_derives_from_sends_no_kind_unless_the_caller_names_one():
    requests, transport = recording(202, ACCEPTED)
    with client(transport) as oml:
        oml.write("Derived without a kind.", derives_from=["art_0"])
        oml.write("Derived as learn stored it.", derives_from=["art_0"], kind="derived_fact")
    assert [json.loads(r.content) for r in requests] == [
        {"body": "Derived without a kind.", "derives_from": ["art_0"]},
        {"body": "Derived as learn stored it.", "derives_from": ["art_0"], "kind": "derived_fact"},
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


def test_withdraw_posts_the_reason_under_an_idempotency_key():
    result = {
        "artifact_id": "art_1",
        "state": "withdrawn",
        "at": "2026-11-17T18:00:00Z",
        "journal_seq": 7,
    }
    requests, transport = recording(200, result)
    with client(transport) as oml:
        written = oml.withdraw("art_1", reason="written by mistake")
    assert [(r.method, r.url.path) for r in requests] == [("POST", "/v1/memories/art_1/withdraw")]
    assert json.loads(requests[0].content) == {"reason": "written by mistake"}
    assert requests[0].headers["idempotency-key"] == written.idempotency_key
    assert written.value.state == "withdrawn"


def served_skill(name: str, depth: int = 0) -> dict:
    """One offered skill as `GET /v1/skills` serves it (openapi.yaml `ServedSkill`)."""
    return {
        "artifact_id": f"art_{name}",
        "claim_key": f"skill/{name}",
        "name": name,
        "description": f"The {name} routine.",
        "version": 1,
        "workspace_path": "universe.alice",
        "depth": depth,
        "overrides": [],
        "approved_at": "2026-11-17T18:00:00Z",
        "approver": {"used_by": "person", "same_user": False},
        "uri": f"oml://workspace/universe.alice/skill/{name}",
        "unreviewed": False,
    }


def skill_page(names: list[str], next_cursor: str | None) -> dict:
    return {
        "scope_node": "ws_1",
        "scope_path": "universe.alice",
        "guardrail_scope": "ancestors",
        "set_digest": "sd_1",
        "skills": [served_skill(name) for name in names],
        "next_cursor": next_cursor,
    }


def test_skills_reads_one_page_of_the_offered_set_and_sends_only_the_fields_given():
    requests, transport = recording(200, skill_page(["deploy"], "c2"))
    with client(transport) as oml:
        page = oml.skills()
        oml.skills("universe.alice", include_own_held=True, cursor="c 1")
    assert [(r.method, r.url.path) for r in requests] == [("GET", "/v1/skills")] * 2
    assert [dict(r.url.params) for r in requests] == [
        {},
        {"workspace": "universe.alice", "include_own_held": "true", "cursor": "c 1"},
    ]
    assert [s.name for s in page.skills] == ["deploy"]
    assert page.next_cursor == "c2"
    assert page.skills[0].uri == "oml://workspace/universe.alice/skill/deploy"


def test_iter_skills_follows_next_cursor_across_pages_until_it_is_null():
    answers = [skill_page(["cite", "deploy"], "c2"), skill_page(["triage"], None)]
    requests: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        requests.append(request)
        return httpx.Response(200, json=answers[len(requests) - 1])

    with client(httpx.MockTransport(handler)) as oml:
        names = [s.name for s in oml.iter_skills("universe.alice", include_own_held=True)]
    assert names == ["cite", "deploy", "triage"]
    assert [dict(r.url.params) for r in requests] == [
        {"workspace": "universe.alice", "include_own_held": "true"},
        {"workspace": "universe.alice", "include_own_held": "true", "cursor": "c2"},
    ]


def test_write_sends_a_skills_name_and_description_and_a_documents_title():
    requests, transport = recording(202, ACCEPTED)
    with client(transport) as oml:
        oml.write(
            "Run the tests, then tag.",
            kind="skill",
            skill={"name": "release", "description": "Cut a release."},
        )
        oml.write("# Runbook\n\nRestart the lead.", kind="document", document={"title": "Runbook"})
        oml.write(
            "Run the tests twice, then tag.",
            replaces="art_1",
            expected_version=1,
            skill={"name": "release", "description": "Cut a release."},
        )
    assert [(r.method, r.url.path) for r in requests] == [
        ("POST", "/v1/memories"),
        ("POST", "/v1/memories"),
        ("POST", "/v1/memories/art_1/supersede"),
    ]
    assert [json.loads(r.content) for r in requests] == [
        {
            "body": "Run the tests, then tag.",
            "kind": "skill",
            "skill": {"name": "release", "description": "Cut a release."},
        },
        {
            "body": "# Runbook\n\nRestart the lead.",
            "kind": "document",
            "document": {"title": "Runbook"},
        },
        {
            "body": "Run the tests twice, then tag.",
            "expected_version": 1,
            "skill": {"name": "release", "description": "Cut a release."},
        },
    ]


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


def test_workspace_without_a_name_reads_the_keys_default_workspace(oml_server, agent):
    view = agent.workspace()
    assert view.id == oml_server.workspace
    assert "remember" in view.my_effective_verbs
    assert agent.workspace(oml_server.review_workspace).id == oml_server.review_workspace


def test_events_tails_the_named_workspace_from_after_seq(oml_server, agent, marker):
    # The key's own workspace, never the review queue's, which the agent loop reads whole.
    written = agent.write(f"An event to tail, {marker}.", workspace=oml_server.workspace)
    page = agent.events(workspace=oml_server.workspace)
    assert page.head_seq >= written.value.journal_seq
    later = agent.events(workspace=oml_server.workspace, after_seq=page.head_seq)
    assert later.items == []


def test_fence_for_names_the_markdown_fence_of_each_media_type():
    from oml_sdk import fence_for

    assert fence_for("text/vnd.oml.mermaid") == "mermaid"
    assert fence_for("application/json") == "json"
    assert fence_for("application/yaml") == "yaml"
    assert fence_for("text/vnd.oml.code; lang=rust") == "rust"
    assert fence_for("text/vnd.oml.code;lang=python") == "python"
    assert fence_for("text/markdown") == "markdown"
    assert fence_for("text/plain") == ""
    assert fence_for(None) == ""
    assert fence_for("application/x-unknown") == ""
