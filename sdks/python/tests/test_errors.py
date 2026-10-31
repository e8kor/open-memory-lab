"""A typed exception per errors.md code family (docs/engineering/sdks.md)."""

import json
import re
import uuid
from pathlib import Path

import pytest

from oml_sdk import (
    AccessError,
    ApiError,
    AuthError,
    OmlClient,
    RequestError,
    SubmissionError,
)
from oml_sdk.errors import FAMILIES, error_from_response

ERRORS_MD = Path(__file__).resolve().parents[3] / "docs" / "api" / "errors.md"


def declared_families() -> set[str]:
    """Every family a code in an errors.md family table belongs to."""
    families, in_family_table = set(), False
    for line in ERRORS_MD.read_text().splitlines():
        if line.startswith("| Code | HTTP |"):
            in_family_table = True
        elif not line.startswith("|"):
            in_family_table = False
        elif in_family_table:
            match = re.match(r"\| `([a-z_]+)\.[a-z_]+` \|", line)
            if match:
                families.add(match.group(1))
    return families


def envelope(code: str) -> bytes:
    return json.dumps(
        {
            "error": {"code": code, "message": "m", "rule": "r", "retryable": False},
            "request_id": "req_1",
        }
    ).encode()


def test_every_family_errors_md_declares_has_its_own_exception_class():
    families = declared_families()
    assert "auth" in families and "system" in families
    assert set(FAMILIES) == families
    assert len(set(FAMILIES.values())) == len(FAMILIES)
    assert all(issubclass(cls, ApiError) for cls in FAMILIES.values())


def test_a_bad_key_is_an_auth_error_with_its_code_rule_message_and_retryable(oml_server):
    with (
        OmlClient(base_url=oml_server.url, api_key="oml_not_a_key") as client,
        pytest.raises(AuthError) as caught,
    ):
        client.recall("anything")
    error = caught.value
    assert (error.code, error.status, error.retryable) == ("auth.invalid", 401, False)
    assert error.rule and error.message
    assert error.request_id.startswith("req_")
    assert "oml_not_a_key" not in str(error) and "oml_not_a_key" not in repr(error)


def test_a_writer_key_asking_for_review_is_an_access_error(agent):
    with pytest.raises(AccessError) as caught:
        agent.review_next()
    assert (caught.value.code, caught.value.status) == ("access.verb_ceiling", 403)
    assert caught.value.rule


def test_learn_without_lineage_is_a_submission_error(agent):
    with pytest.raises(SubmissionError) as caught:
        agent.learn("no-lineage", "A derived claim that names no ancestor.", derives_from=[])
    assert (caught.value.code, caught.value.status) == ("submission.lineage_required", 422)
    assert caught.value.retryable is False


def test_a_memory_the_key_cannot_see_is_access_not_found(agent):
    with pytest.raises(AccessError) as caught:
        agent.get(str(uuid.uuid4()))
    assert (caught.value.code, caught.value.status) == ("access.not_found", 404)


def test_a_budget_without_bodies_is_a_request_error(agent):
    with pytest.raises(RequestError) as caught:
        agent.recall("anything", max_tokens=100, include_bodies=False)
    assert caught.value.code == "request.validation"


def test_a_code_the_client_has_never_seen_lands_in_its_familys_class():
    error = error_from_response(409, envelope("submission.brand_new"), {})
    assert type(error) is SubmissionError
    assert error.code == "submission.brand_new"


def test_a_family_the_client_has_never_seen_lands_in_api_error():
    error = error_from_response(409, envelope("brand.new"), {})
    assert type(error) is ApiError
    assert error.code == "brand.new"


def test_a_response_without_the_envelope_is_an_api_error_without_a_code():
    error = error_from_response(502, b"<html>bad gateway</html>", {})
    assert type(error) is ApiError
    assert (error.code, error.status, error.retryable) == (None, 502, False)
