"""The client starts from one URL-and-key pair: the arguments, else OML_URL and
OML_API_KEY, taken from the first source that sets either and refused naming
the other when that source sets one alone (ADR-0039 d15)."""

import importlib.metadata

import httpx
import pytest

from oml_sdk import ApiError, ConfigError, OmlClient


def recorder() -> tuple[list[httpx.Request], httpx.MockTransport]:
    requests: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        requests.append(request)
        return httpx.Response(500)

    return requests, httpx.MockTransport(handler)


def test_the_key_and_url_come_from_the_environment_when_neither_is_passed(oml_server, monkeypatch):
    monkeypatch.setenv("OML_API_KEY", oml_server.agent_key)
    monkeypatch.setenv("OML_URL", oml_server.url)
    with OmlClient() as client:
        page = client.read(limit=1)
    assert isinstance(page.items, list)


def test_the_arguments_win_over_the_environment(oml_server, monkeypatch):
    monkeypatch.setenv("OML_API_KEY", "oml_not_a_key")
    monkeypatch.setenv("OML_URL", "http://127.0.0.1:9")
    with OmlClient(url=oml_server.url, api_key=oml_server.agent_key) as client:
        client.read(limit=1)


def test_the_client_refuses_an_environment_that_sets_only_oml_url(monkeypatch):
    monkeypatch.setenv("OML_URL", "http://oml.test")
    requests, transport = recorder()
    with pytest.raises(ConfigError, match="OML_URL is set without OML_API_KEY"):
        OmlClient(transport=transport)
    assert requests == []


def test_the_client_refuses_an_environment_that_sets_only_oml_api_key(monkeypatch):
    monkeypatch.setenv("OML_API_KEY", "oml_test_key")
    with pytest.raises(ConfigError, match="OML_API_KEY is set without OML_URL"):
        OmlClient()


def test_one_argument_alone_is_refused_and_never_takes_the_other_from_the_environment(monkeypatch):
    monkeypatch.setenv("OML_URL", "http://oml.test")
    monkeypatch.setenv("OML_API_KEY", "oml_test_key")
    requests, transport = recorder()
    with pytest.raises(ConfigError, match="url is passed without api_key"):
        OmlClient(url="http://elsewhere.test", transport=transport)
    with pytest.raises(ConfigError, match="api_key is passed without url"):
        OmlClient(api_key="oml_other_key", transport=transport)
    assert requests == []


def test_a_client_with_no_source_refuses_before_any_request():
    requests, transport = recorder()
    with pytest.raises(ConfigError, match="no URL and API key"):
        OmlClient(transport=transport)
    assert requests == []


def test_the_key_is_in_no_repr():
    client = OmlClient(url="http://oml.test", api_key="oml_secret_value")
    assert "oml_secret_value" not in repr(client)
    assert "oml_secret_value" not in str(client)
    client.close()


def test_every_request_names_the_sdk_and_its_version_in_user_agent():
    # The server's request log names the client by its User-Agent (ADR-0042 d9).
    requests, transport = recorder()
    with (
        OmlClient(
            url="http://oml.test", api_key="oml_sk_x", retries=0, transport=transport
        ) as client,
        pytest.raises(ApiError),
    ):
        client.read(limit=1)
    version = importlib.metadata.version("oml-sdk")
    assert requests and requests[0].headers["user-agent"] == f"oml-sdk-python/{version}"
