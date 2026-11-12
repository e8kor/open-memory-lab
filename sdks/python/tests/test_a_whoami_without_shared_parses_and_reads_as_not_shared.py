"""A whoami without `shared`, as a server older than ADR-0076 answers, parses,
and its `shared` is unset, which a client reads as not shared (ADR-0076 d9, the
operator's clarification of 2026-11-11). Needs no server."""

from oml_sdk.generated.models.whoami import Whoami
from oml_sdk.generated.types import UNSET

OLDER = {
    "key_id": "key_w",
    "name": "writer",
    "role": "writer",
    "principal": "p",
    "held_by": "agent",
    "tenant": {"id": "t1", "name": "oml"},
    "default_workarea": None,
    "crew": None,
    "user": None,
    "grants": [],
}


def test_a_whoami_without_shared_parses_and_reads_as_not_shared() -> None:
    me = Whoami.from_dict(dict(OLDER))
    assert me.shared is UNSET
    assert "shared" not in me.to_dict()


def test_a_whoami_with_shared_carries_it() -> None:
    assert Whoami.from_dict({**OLDER, "shared": True}).shared is True
    assert Whoami.from_dict({**OLDER, "shared": False}).shared is False
