"""A real oml-server-lead on a throwaway database, for the SDK's tests.

docs/engineering/sdks.md, "Tests": mint a uniquely named owner/serving role
pair and a database the owner owns, boot the server on it, migrating under the
owner and serving under the serving role (ADR-0034 d2, d4, d8), with a keyring
in a scratch directory, read the enrollment token from its stdout, claim the
bootstrap and issue the keys with the `oml` CLI, and drop the database and the
pair afterwards.

Minting takes a superuser. When the admin role is not one, as a role holding
CREATEDB alone is not, the suite's server tests skip with
`did not run: <role> cannot create roles`; under OML_TEST_REQUIRE_SPLIT=1,
which CI sets, the same condition fails them, so no run passes vacuously.

Environment (all optional):
  OML_SERVER_BIN              the lead binary (default: oml-server-lead on PATH)
  OML_CLI_BIN                 the CLI binary (default: oml on PATH)
  OML_SDK_TEST_DATABASE_URL   a superuser that can mint roles
                              (required: a throwaway cluster; no default)
  OML_SDK_TEST_PORT           the port to listen on (default: one the OS hands out)
  OML_TEST_REQUIRE_SPLIT      1: fail, never skip, when the admin role cannot mint

The server and the CLI get an environment built from an allow-list, so a key
in the caller's environment never reaches either.
"""

from __future__ import annotations

import dataclasses
import json
import os
import re
import secrets
import shutil
import socket
import subprocess
import time
import urllib.parse
from collections.abc import Iterator
from pathlib import Path

import httpx
import pytest

# No default: a suite that guessed a URL could reach a live server on the
# conventional port. Unset, every test needing it refuses (see _require_minting).
ADMIN_URL = os.environ.get("OML_SDK_TEST_DATABASE_URL", "")
SERVER_BIN = os.environ.get("OML_SERVER_BIN", "oml-server-lead")
CLI_BIN = os.environ.get("OML_CLI_BIN", "oml")
TOKEN_RE = re.compile(r"enr_[0-9a-f]{48}")
ALLOWED_ENV = ("PATH", "HOME", "LANG", "LC_ALL", "TZ", "TMPDIR")


@dataclasses.dataclass(frozen=True)
class OmlServer:
    url: str
    agent_key: str
    dreamer_key: str
    workarea: str
    review_workarea: str

    def __repr__(self) -> str:  # never the key values
        return f"OmlServer(url={self.url!r})"


def _clean_env(**extra: str) -> dict[str, str]:
    env = {name: os.environ[name] for name in ALLOWED_ENV if name in os.environ}
    env.update(extra)
    return env


def _psql(sql: str) -> str:
    done = subprocess.run(
        ["psql", ADMIN_URL, "-v", "ON_ERROR_STOP=1", "-q", "-t", "-A", "-c", sql],
        env=_clean_env(),
        capture_output=True,
        text=True,
        check=False,
    )
    if done.returncode != 0:  # stderr only: the statement may carry a password
        pytest.fail(f"psql could not run a statement (is PostgreSQL up?):\n{done.stderr}")
    return done.stdout.strip()


# Two columns, so psql -tA prints the boolean in its display form, `t` or `f`,
# joined to the name by `|`; concatenated to text it would read `true`/`false`.
ADMIN_ROLE_QUERY = "SELECT current_user, rolsuper FROM pg_roles WHERE rolname = current_user"


def _admin_role(query: str = ADMIN_ROLE_QUERY) -> tuple[str, bool]:
    """The admin role's name, and whether it is a superuser that can mint (ADR-0034 d8)."""
    role, superuser = _psql(query).rsplit("|", 1)
    return role, superuser == "t"


def _require_minting() -> None:
    """Skip, or under OML_TEST_REQUIRE_SPLIT=1 fail, when the admin role cannot mint (ADR-0034 d8)."""
    if not ADMIN_URL:
        if os.environ.get("OML_TEST_REQUIRE_SPLIT") == "1":
            pytest.fail("OML_SDK_TEST_DATABASE_URL is unset: name a throwaway cluster")
        pytest.skip("did not run: set OML_SDK_TEST_DATABASE_URL to a throwaway cluster")
    role, superuser = _admin_role()
    if superuser:
        return
    if os.environ.get("OML_TEST_REQUIRE_SPLIT") == "1":
        pytest.fail(
            f"{role} cannot create roles, and OML_TEST_REQUIRE_SPLIT=1 requires the role pair (ADR-0034 d8)"
        )
    pytest.skip(f"did not run: {role} cannot create roles")


def _database_url(name: str, role: str, password: str) -> str:
    parts = urllib.parse.urlsplit(ADMIN_URL)
    netloc = f"{role}:{password}@{parts.hostname}" + (f":{parts.port}" if parts.port else "")
    return urllib.parse.urlunsplit(parts._replace(netloc=netloc, path=f"/{name}"))


def _port() -> int:
    if "OML_SDK_TEST_PORT" in os.environ:
        return int(os.environ["OML_SDK_TEST_PORT"])
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _tail(path: Path) -> str:
    try:
        return "\n".join(path.read_text(errors="replace").splitlines()[-20:])
    except OSError:
        return ""


@pytest.fixture(scope="session")
def oml_server(tmp_path_factory: pytest.TempPathFactory) -> Iterator[OmlServer]:
    _require_minting()
    scratch = tmp_path_factory.mktemp("oml-server")
    database = f"oml_sdk_py_{secrets.token_hex(6)}"
    owner, serving = f"{database}_owner", f"{database}_serving"
    password = secrets.token_hex(12)
    port = _port()
    url = f"http://127.0.0.1:{port}"
    stdout_path, stderr_path = scratch / "server.stdout", scratch / "server.stderr"

    # One statement string is one transaction: both roles exist, or neither.
    _psql(
        f"CREATE ROLE {owner} LOGIN PASSWORD '{password}'; "
        f"CREATE ROLE {serving} LOGIN PASSWORD '{password}'"
    )
    server = None
    try:
        _psql(f"CREATE DATABASE {database} OWNER {owner}")
        with stdout_path.open("wb") as out, stderr_path.open("wb") as err:
            server = subprocess.Popen(
                [SERVER_BIN],
                stdout=out,
                stderr=err,
                env=_clean_env(
                    OML_POSTGRES__URL=_database_url(database, serving, password),
                    OML_POSTGRES__OWNER_URL=_database_url(database, owner, password),
                    OML_SERVER__LISTEN=f"127.0.0.1:{port}",
                    OML_KMS__KEYRING_PATH=str(scratch / "keyring"),
                ),
            )
        token = None
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if server.poll() is not None:
                pytest.fail(f"oml-server-lead exited during boot:\n{_tail(stderr_path)}")
            match = TOKEN_RE.search(stdout_path.read_text(errors="replace"))
            if match and token is None:
                token = match.group(0)
            if token is not None:
                try:
                    if httpx.get(f"{url}/readyz", timeout=1).status_code == 200:
                        break
                except httpx.TransportError:
                    pass
            time.sleep(0.1)
        else:
            pytest.fail(f"oml-server-lead never became ready on {url}:\n{_tail(stderr_path)}")

        cli_env = _clean_env(OML_CLI_CREDENTIALS=str(scratch / "credentials"))

        def oml(*args: str, stdin: str = "") -> dict:
            # The claim takes the URL alone; every verb after it, the URL and
            # key the claim stored (ADR-0039 d15).
            at = ["--url", url] if args[0] == "bootstrap" else []
            done = subprocess.run(
                [CLI_BIN, *at, "--json", *args],
                env=cli_env,
                input=stdin,
                capture_output=True,
                text=True,
                check=False,
            )
            if done.returncode != 0:  # stderr only: stdout may hold a key value
                pytest.fail(f"oml {args[0]} {args[1]} failed:\n{done.stderr}")
            return json.loads(done.stdout)

        boot = oml(
            "bootstrap", "claim", "sdk-admin", "--token-stdin",
            "--tenant-name", "sdk-tests", "--principal", "platform/admin",
            stdin=f"{token}\n",
        )  # fmt: skip
        root = boot["root_workarea_id"]
        workarea = oml("workareas", "create", root, "sdk")["id"]
        review_workarea = oml("workareas", "create", root, "sdk-review")["id"]
        agent = oml(
            "keys", "issue", "sdk-agent", "--role", "writer", "--held-by", "agent",
            "--principal", "sdk/agent", "--default-workarea", workarea,
            "--grant", f"{workarea}:writer", "--grant", f"{workarea}:reader",
            "--grant", f"{review_workarea}:writer", "--grant", f"{review_workarea}:reader",
        )["value"]  # fmt: skip
        dreamer = oml(
            "keys", "issue", "sdk-reviewer", "--role", "dreamer", "--held-by", "human",
            "--principal", "sdk/reviewer", "--default-workarea", review_workarea,
            "--grant", f"{review_workarea}:dreamer",
        )["value"]  # fmt: skip
        yield OmlServer(url, agent, dreamer, workarea, review_workarea)
    finally:
        if server is not None and server.poll() is None:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
        _psql(f"DROP DATABASE IF EXISTS {database} WITH (FORCE)")
        _psql(f"DROP ROLE IF EXISTS {owner}")
        _psql(f"DROP ROLE IF EXISTS {serving}")
        shutil.rmtree(scratch, ignore_errors=True)


@pytest.fixture(autouse=True)
def _no_inherited_key(monkeypatch: pytest.MonkeyPatch) -> None:
    """No test reads a key or a server from the environment it inherited."""
    monkeypatch.delenv("OML_API_KEY", raising=False)
    monkeypatch.delenv("OML_URL", raising=False)


@pytest.fixture
def agent(oml_server: OmlServer):
    from oml_sdk import OmlClient

    with OmlClient(url=oml_server.url, api_key=oml_server.agent_key) as client:
        yield client


@pytest.fixture
def reviewer(oml_server: OmlServer):
    from oml_sdk import OmlClient

    with OmlClient(url=oml_server.url, api_key=oml_server.dreamer_key) as client:
        yield client


@pytest.fixture
def marker() -> str:
    """A word no other test writes, for recall queries."""
    return "sdkmark" + "".join(secrets.choice("abcdefghijklmnopqrstuvwxyz") for _ in range(10))
