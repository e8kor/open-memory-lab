"""The harness's minting check, read through psql as it runs it (ADR-0034 d8).

The admin role here is rarely a superuser, so the check's two answers are
proved by putting a literal boolean where `rolsuper` stands: the same query,
the same psql flags, the same parse.
"""

from conftest import ADMIN_ROLE_QUERY, _admin_role, _psql


def test_a_superuser_admin_role_is_read_as_one_that_can_mint() -> None:
    assert "rolsuper" in ADMIN_ROLE_QUERY
    _, superuser = _admin_role(ADMIN_ROLE_QUERY.replace("rolsuper", "true"))
    assert superuser is True


def test_any_other_admin_role_is_read_as_one_that_cannot_mint() -> None:
    _, superuser = _admin_role(ADMIN_ROLE_QUERY.replace("rolsuper", "false"))
    assert superuser is False


def test_the_admin_role_is_read_by_its_name() -> None:
    role, _ = _admin_role(ADMIN_ROLE_QUERY.replace("rolsuper", "false"))
    assert role == _psql("SELECT current_user")
