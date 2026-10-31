"""The key comes from the caller or OML_API_KEY, and nowhere else (docs/engineering/sdks.md)."""

import httpx
import pytest

from oml_sdk import AuthError, ConfigError, OmlClient


def recorder() -> tuple[list[httpx.Request], httpx.MockTransport]:
    requests: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        requests.append(request)
        return httpx.Response(500)

    return requests, httpx.MockTransport(handler)


def test_the_key_and_server_come_from_the_environment_when_none_is_passed(oml_server, monkeypatch):
    monkeypatch.setenv("OML_API_KEY", oml_server.agent_key)
    monkeypatch.setenv("OML_URL", oml_server.url)
    with OmlClient() as client:
        page = client.list_memories(own=True, limit=1)
    assert isinstance(page.items, list)


def test_an_explicit_key_wins_over_oml_api_key(oml_server, monkeypatch):
    monkeypatch.setenv("OML_API_KEY", "oml_not_a_key")
    with OmlClient(base_url=oml_server.url, api_key=oml_server.agent_key) as client:
        client.list_memories(own=True, limit=1)
    with OmlClient(base_url=oml_server.url) as client, pytest.raises(AuthError):
        client.list_memories(own=True, limit=1)


def test_a_client_without_a_key_refuses_before_any_request():
    requests, transport = recorder()
    with pytest.raises(ConfigError):
        OmlClient(base_url="http://oml.test", transport=transport)
    assert requests == []


def test_a_client_without_a_server_refuses_before_any_request():
    requests, transport = recorder()
    with pytest.raises(ConfigError):
        OmlClient(api_key="oml_test_key", transport=transport)
    assert requests == []


def test_the_key_is_in_no_repr():
    client = OmlClient(base_url="http://oml.test", api_key="oml_secret_value")
    assert "oml_secret_value" not in repr(client)
    assert "oml_secret_value" not in str(client)
    client.close()
