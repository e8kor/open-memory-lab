"""Uploads are a stub until the server serves upload sessions (docs/engineering/sdks.md)."""

import httpx
import pytest

from oml_sdk import NotBuiltError, OmlClient


def test_an_upload_raises_not_built_and_sends_no_request():
    requests: list[httpx.Request] = []

    def handler(request: httpx.Request) -> httpx.Response:
        requests.append(request)
        return httpx.Response(500)

    with (
        OmlClient(
            base_url="http://oml.test",
            api_key="oml_test_key",
            transport=httpx.MockTransport(handler),
        ) as client,
        pytest.raises(NotBuiltError),
    ):
        client.upload_attachment(b"%PDF-1.7", media_type="application/pdf")
    assert requests == []
