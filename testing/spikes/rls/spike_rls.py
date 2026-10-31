#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "psycopg[binary]==3.3.6",
#     "psycopg-pool==3.3.3",
# ]
# ///
"""M0 spike 3: RLS hardening, scenarios S3.1-S3.4 of docs/product/critical-path.md.

    uv run --locked --script testing/spikes/rls/spike_rls.py [--dsn DSN] [--require-all]

The harness decisions (roles, seeding, controls) are on docs/engineering/spikes/index.md.
The recorded findings are on docs/engineering/spikes/spike-3-rls.md.

What it does, in order:

1. Connects with --dsn (or OML_SPIKE_PG_DSN, or the local dev role) and creates a throwaway
   database with a unique name. If the connecting role is a superuser, it also mints an owner
   role and a service role (both NOSUPERUSER NOBYPASSRLS), and the owner role owns the
   database. Otherwise the connecting role owns the database and S3.2 is not run.
2. Applies migrations/*.sql in filename order under the owner role. Two tenants are seeded
   after 0003, so the backfills of 0004 and 0005 run over both tenants' rows while FORCE ROW
   LEVEL SECURITY binds the role running them. A table that a later migration adds and no
   seeder writes is checked for ENABLE, FORCE and a policy, and reported as unprobed for row
   visibility; it does not fail the run.
3. Runs S3.1-S3.4. Each scenario has checks, which must hold, and controls, which weaken one
   thing and must then observe the leak. A scenario whose control does not leak is
   inconclusive.
4. Prints one finding per scenario in the critical path's template, drops the database and
   the roles whatever the result, and exits 0 only if every scenario that ran passed (and,
   with --require-all, every scenario ran).

It never reads OML_API_KEY, never escalates, and writes nothing outside its database.
"""

from __future__ import annotations

import argparse
import contextlib
import logging
import hashlib
import json
import os
import platform
import re
import secrets
import sys
import uuid
from dataclasses import dataclass, field
from datetime import date
from pathlib import Path
from typing import Any, Iterator

try:
    import psycopg
    import psycopg_pool
    from psycopg import errors, sql
    from psycopg.conninfo import conninfo_to_dict, make_conninfo
    from psycopg.types.json import Jsonb
    from psycopg_pool import ConnectionPool
except ImportError as missing:
    sys.exit(
        f"spike_rls: {missing.name} is not importable; run "
        "`uv run --locked --script testing/spikes/rls/spike_rls.py`"
    )

ROOT = Path(__file__).resolve().parents[3]
MIGRATIONS = ROOT / "migrations"
DEFAULT_DSN = "postgresql://oml:oml_dev_only@127.0.0.1:5432/oml_dev"
SEED_BEFORE = "0004"  # seed both tenants after 0003, before this migration

NS = uuid.UUID("6f6d6c2d-7370-696b-6533-2d726c732d31")
MARKER = {"A": "tenant-a-visible", "B": "TENANT-B-SECRET"}

# The adapter's column lists at commit 02a385d (crates/adapters/oml-pg/src/lib.rs).
MEMORY_COLUMNS = (
    "m.id, m.tenant_id, m.workarea_id, m.claim_key, m.version, m.kind, m.claim, "
    "m.claim_digest, m.body_bytes, m.status, m.is_tip, m.retirement, m.replaces_artifact_id, "
    "m.author_principal, m.author_key_id, m.derives_from, m.lineage_principals, "
    "m.lineage_key_ids, m.scan_finding, "
    "to_char(m.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created_at_signed, "
    "to_char(m.promoted_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS promoted_at_signed"
)
WORKAREA_COLUMNS = "id, tenant_id, parent_id, name, crew"

# Failed checks recorded as findings on docs/engineering/spikes/spike-3-rls.md. A listed
# check that fails is reported as recorded and does not fail the run. A listed check that
# passes, or does not run, fails the run, so a finding is re-recorded instead of going stale.
RECORDED_FAILURES = {
    "S3.1/owner/pg_stats": "spike-3-rls.md#s31",
    "S3.4/owner/pg_stats": "spike-3-rls.md#s34",
}

# The tables the seeders below wrote rows into, filled by seed_insert as they run. A table a
# later migration adds and no seeder writes is probed structurally (ENABLE, FORCE, a policy,
# zero rows with no tenant) and reported as unprobed for row visibility: no row of either
# tenant exists in it, so no read of it could show a leak (docs/engineering/spikes/index.md).
SEEDED_TABLES: set[str] = set()


# ---------------------------------------------------------------------------
# Results
# ---------------------------------------------------------------------------


@dataclass
class Scenario:
    id: str
    title: str
    adr_clause: str
    ran: bool = True
    not_run_reason: str = ""
    checks: list[tuple[bool, str, str | None]] = field(default_factory=list)
    controls: list[tuple[bool, str]] = field(default_factory=list)
    observations: list[str] = field(default_factory=list)
    settings: list[str] = field(default_factory=list)
    unprobed: list[str] = field(default_factory=list)  # tables with no seeded row (SEEDED_TABLES)

    def check(self, ok: bool, text: str, key: str | None = None) -> bool:
        """key names a check RECORDED_FAILURES may list; it is "<scenario>/<role kind>/<what>"."""
        self.checks.append((bool(ok), text, key))
        return bool(ok)

    def control(self, leaked: bool, text: str) -> None:
        self.controls.append((bool(leaked), text))

    @property
    def verdict(self) -> str:
        if not self.ran:
            return "not run"
        if not all(ok for ok, _, _ in self.checks):
            return "fail"
        if not self.controls or not all(ok for ok, _ in self.controls):
            return "inconclusive"
        return "pass"


# ---------------------------------------------------------------------------
# Fixture: two tenants, every table populated
# ---------------------------------------------------------------------------


def uid(tenant: str, kind: str, n: int = 1) -> uuid.UUID:
    return uuid.uuid5(NS, f"{tenant}/{kind}/{n}")


def key_hash(tenant: str, role: str) -> bytes:
    return hashlib.sha256(f"{MARKER[tenant]}-{role}-key".encode()).digest()


@dataclass
class Tenant:
    letter: str
    id: uuid.UUID
    universe: uuid.UUID
    child: uuid.UUID
    writer: uuid.UUID
    dreamer: uuid.UUID
    m1: uuid.UUID  # promoted memory
    m2: uuid.UUID  # held memory, in review_queue
    m3: uuid.UUID  # promoted steering (kind set after 0005)
    m4: uuid.UUID  # held successor of m1, inserted after 0005
    grant_w: uuid.UUID
    grant_d: uuid.UUID
    denial: uuid.UUID
    idem_key: str
    marker: str
    principal: str
    events: dict[int, uuid.UUID]

    @property
    def nodes(self) -> list[uuid.UUID]:
        return [self.universe, self.child]

    @property
    def memories(self) -> list[uuid.UUID]:
        return [self.m1, self.m2, self.m3, self.m4]

    def identifiers(self) -> set[str]:
        ids = {self.id, self.universe, self.child, self.writer, self.dreamer, self.grant_w,
               self.grant_d, self.denial, *self.memories, *self.events.values()}
        return {str(i) for i in ids} | {self.marker, self.idem_key}


def make_tenant(letter: str) -> Tenant:
    t = Tenant(
        letter=letter,
        id=uid(letter, "tenant"),
        universe=uid(letter, "workarea", 0),
        child=uid(letter, "workarea", 1),
        writer=uid(letter, "key", 1),
        dreamer=uid(letter, "key", 2),
        m1=uid(letter, "memory", 1),
        m2=uid(letter, "memory", 2),
        m3=uid(letter, "memory", 3),
        m4=uid(letter, "memory", 4),
        grant_w=uid(letter, "grant", 1),
        grant_d=uid(letter, "grant", 2),
        denial=uid(letter, "denial", 1),
        idem_key="",
        marker=MARKER[letter],
        principal=f"{MARKER[letter]}/agent-1",
        events={seq: uid(letter, "event", seq) for seq in range(1, 10)},
    )
    t.idem_key = f"{t.writer}:{t.child}:{t.marker}-idem"
    return t


def set_tenant(conn: psycopg.Connection, tenant: uuid.UUID | None) -> None:
    """The adapter's form: set_config(..., is_local = true)."""
    if tenant is not None:
        conn.execute("SELECT set_config('oml.tenant_id', %s, true)", [str(tenant)])


@contextlib.contextmanager
def tenant_tx(conn: psycopg.Connection, tenant: uuid.UUID | None) -> Iterator[psycopg.Connection]:
    with conn.transaction():
        set_tenant(conn, tenant)
        yield conn


INSERT_TARGET = re.compile(r"\s*INSERT INTO (\w+)")


def seed_insert(conn: psycopg.Connection, statement: str, params: list) -> None:
    """Every seed row goes in through here, so SEEDED_TABLES names exactly the tables seeded."""
    SEEDED_TABLES.add(INSERT_TARGET.match(statement).group(1))
    conn.execute(statement, params)


def unseeded(tables: dict[str, str]) -> list[str]:
    return sorted(set(tables) - SEEDED_TABLES)


def journal_row(conn, t: Tenant, seq: int, event_type: str, payload: dict, key: uuid.UUID | None):
    signed = json.dumps({"seq": seq, "event_type": event_type, "payload": payload,
                         "note": t.marker}, sort_keys=True).encode()
    prev = b"\x00" * 32 if seq == 1 else hashlib.sha256(f"{t.letter}-{seq - 1}".encode()).digest()
    seed_insert(
        conn,
        "INSERT INTO journal (tenant_id, seq, event_id, event_type, prev_hash, principal, "
        "api_key_id, payload, signed_bytes_hash, sig, signed_bytes) "
        "VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)",
        [t.id, seq, t.events[seq], event_type, prev, t.principal, key,
         Jsonb(payload), hashlib.sha256(signed).digest(), b"\x00" * 64, signed],
    )


def seed_before_0004(conn: psycopg.Connection, t: Tenant) -> None:
    """Rows in every 0001-0003 table, in the 0003 schema (0001 + 0002 columns)."""
    with tenant_tx(conn, t.id):
        seed_insert(conn, "INSERT INTO tenants (id, name) VALUES (%s, %s)", [t.id, f"{t.marker} tenant"])
        seed_insert(
            conn, "INSERT INTO workareas (id, tenant_id, parent_id, name, status) VALUES "
            "(%s, %s, NULL, %s, 'committed'), (%s, %s, %s, %s, 'development')",
            [t.universe, t.id, f"{t.marker} universe", t.child, t.id, t.universe, f"{t.marker} child"],
        )
        seed_insert(
            conn, "INSERT INTO api_keys (id, tenant_id, hash, role, principal, held_by, default_workarea_id) "
            "VALUES (%s, %s, %s, 'writer', %s, 'agent', %s), (%s, %s, %s, 'dreamer', %s, 'agent', %s)",
            [t.writer, t.id, key_hash(t.letter, "writer"), t.principal, t.child,
             t.dreamer, t.id, key_hash(t.letter, "dreamer"), f"{t.marker}/dreamer-1", t.universe],
        )
        journal_row(conn, t, 1, "tenant.genesis", {"name": t.marker}, None)
        journal_row(conn, t, 2, "key.issued", {"key_id": str(t.writer)}, None)
        journal_row(conn, t, 3, "key.issued", {"key_id": str(t.dreamer)}, t.writer)
        journal_row(conn, t, 4, "submission.remember", {"artifact_id": str(t.m1)}, t.writer)
        journal_row(conn, t, 5, "lifecycle.promote", {"artifact_id": str(t.m1)}, t.dreamer)
        journal_row(conn, t, 6, "submission.remember", {"artifact_id": str(t.m2)}, t.writer)
        journal_row(conn, t, 7, "submission.remember", {"artifact_id": str(t.m3)}, t.writer)
        journal_row(conn, t, 8, "lifecycle.promote", {"artifact_id": str(t.m3)}, t.dreamer)
        seed_insert(
            conn, "INSERT INTO journal_chain_head (tenant_id, seq, head_hash) VALUES (%s, 8, %s)",
            [t.id, hashlib.sha256(f"{t.letter}-8".encode()).digest()],
        )
        for seq in (4, 5, 6, 7, 8):
            seed_insert(
                conn, "INSERT INTO outbox (tenant_id, tenant_seq, kind, payload) VALUES (%s, %s, 'bronze-event', %s)",
                [t.id, seq, Jsonb({"note": t.marker, "seq": seq})],
            )
        memories = [
            (t.m1, f"{t.marker} runbook drain upgrade verify undrain", "promoted", 4, "deploy-runbook"),
            (t.m2, f"{t.marker} held note awaiting review", "held", 6, "held-note"),
            (t.m3, f"{t.marker} steering always drain first", "promoted", 7, "steering-rule"),
        ]
        for mid, claim, status, seq, claim_key in memories:
            seed_insert(
                conn, "INSERT INTO memories (id, tenant_id, workarea_id, claim, claim_digest, status, "
                "created_event_id, terms, claim_key, author_principal, author_key_id, derives_from, "
                "lineage_principals, lineage_key_ids) "
                "VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)",
                [mid, t.id, t.child, claim, hashlib.sha256(claim.encode()).digest(), status,
                 t.events[seq], ["oml:runbook"], claim_key, t.principal, t.writer, [],
                 [t.principal], [t.writer]],
            )
        seed_insert(
            conn, "INSERT INTO review_queue (memory_id, tenant_id, held_by_key) VALUES (%s, %s, NULL)",
            [t.m2, t.id],
        )
        seed_insert(
            conn, "INSERT INTO idempotency (tenant_id, key, request_digest, response, committed_at) "
            "VALUES (%s, %s, %s, %s, now())",
            [t.id, t.idem_key, hashlib.sha256(t.idem_key.encode()).digest(),
             Jsonb({"artifact_id": str(t.m1), "note": t.marker})],
        )


def seed_after_migrations(conn: psycopg.Connection, t: Tenant) -> None:
    """Rows only the 0004/0005 schema has: grants, denials, a steering kind, a held successor."""
    with tenant_tx(conn, t.id):
        seed_insert(
            conn, "INSERT INTO grants (id, tenant_id, key_id, role, workarea_node_id, deny, epoch) VALUES "
            "(%s, %s, %s, 'writer', %s, '[]', 2), (%s, %s, %s, 'dreamer', %s, '[]', 3)",
            [t.grant_w, t.id, t.writer, t.child, t.grant_d, t.id, t.dreamer, t.universe],
        )
        seed_insert(
            conn, "INSERT INTO key_denials (id, tenant_id, key_id, workarea_node_id, epoch, created_by) "
            "VALUES (%s, %s, %s, %s, 3, %s)",
            [t.denial, t.id, t.writer, t.universe, t.dreamer],
        )
        conn.execute("UPDATE memories SET kind = 'steering' WHERE id = %s", [t.m3])
        journal_row(conn, t, 9, "submission.remember", {"artifact_id": str(t.m4)}, t.writer)
        conn.execute("UPDATE journal_chain_head SET seq = 9 WHERE tenant_id = %s", [t.id])
        claim = f"{t.marker} runbook drain upgrade verify undrain, revised"
        seed_insert(
            conn, "INSERT INTO memories (id, tenant_id, workarea_id, claim, claim_digest, status, "
            "created_event_id, claim_key, author_principal, author_key_id, lineage_principals, "
            "lineage_key_ids, version, kind, is_tip, body_bytes, replaces_artifact_id) "
            "VALUES (%s, %s, %s, %s, %s, 'held', %s, 'deploy-runbook', %s, %s, %s, %s, 2, "
            "'memory', false, %s, %s)",
            [t.m4, t.id, t.child, claim, hashlib.sha256(claim.encode()).digest(), t.events[9],
             t.principal, t.writer, [t.principal], [t.writer], len(claim.encode()), t.m1],
        )


# ---------------------------------------------------------------------------
# Helpers over a connection
# ---------------------------------------------------------------------------


def tenant_tables(conn: psycopg.Connection) -> dict[str, str]:
    """Every table in the public schema, with the column that names its tenant."""
    rows = conn.execute(
        "SELECT c.relname, "
        "       CASE WHEN c.relname = 'tenants' THEN 'id' ELSE 'tenant_id' END "
        "  FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace "
        " WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') ORDER BY c.relname"
    ).fetchall()
    return dict(rows)


def tainted(value: Any, needles: set[str]) -> bool:
    """True when any needle appears in value, ignoring case (text search lowercases lexemes)."""
    if value is None:
        return False
    if isinstance(value, (bytes, bytearray, memoryview)):
        b = bytes(value).lower()
        return any(n.lower().encode() in b for n in needles)
    if isinstance(value, dict):
        return any(tainted(k, needles) or tainted(v, needles) for k, v in value.items())
    if isinstance(value, (list, tuple)):
        return any(tainted(v, needles) for v in value)
    text = str(value).lower()
    return any(n.lower() in text for n in needles)


def rows_tainted(rows: list[tuple], needles: set[str]) -> int:
    return sum(1 for r in rows if tainted(r, needles))


def sqlstate_of(conn: psycopg.Connection, statement: str | sql.Composable, params=None) -> str | None:
    """Run statement in a savepoint; return its SQLSTATE, or None when it succeeds."""
    try:
        with conn.transaction():
            conn.execute(statement, params)
        return None
    except psycopg.Error as e:
        return e.sqlstate


def counts(conn: psycopg.Connection, tables: dict[str, str], tenant: uuid.UUID | None,
           other: uuid.UUID) -> dict[str, tuple[int, int]]:
    """Per table: (rows visible, rows visible that belong to `other`)."""
    out = {}
    with tenant_tx(conn, tenant):
        for table, col in tables.items():
            q = sql.SQL("SELECT count(*), count(*) FILTER (WHERE {c} = %s) FROM {t}").format(
                c=sql.Identifier(col), t=sql.Identifier(table))
            out[table] = tuple(conn.execute(q, [other]).fetchone())
    return out


def statistics_rows(conn: psycopg.Connection, tenant: uuid.UUID) -> list[tuple[str, str, str]]:
    """pg_stats for the public schema as (tablename, attname, the whole row as text)."""
    with tenant_tx(conn, tenant):
        return conn.execute(
            "SELECT s.tablename::text, s.attname::text, s::text FROM pg_stats s "
            "WHERE s.schemaname = 'public' ORDER BY 1, 2").fetchall()


def excerpt(text: str, needle: str, width: int = 60) -> str:
    i = text.lower().find(needle.lower())
    return text[max(0, i - width):i + len(needle) + width] if i >= 0 else text[:2 * width]


def connect(conninfo: str, notices: list[str] | None = None) -> psycopg.Connection:
    conn = psycopg.connect(conninfo, autocommit=True)
    if notices is not None:
        conn.add_notice_handler(lambda d: notices.append(f"{d.severity}: {d.message_primary}"))
    return conn


# ---------------------------------------------------------------------------
# S3.1 — FORCE binds the table owner
# ---------------------------------------------------------------------------


def s3_1(owner_ci: str, a: Tenant, b: Tenant, owner_name: str) -> Scenario:
    s = Scenario("S3.1", "FORCE ROW LEVEL SECURITY binds the table owner",
                 "ADR-0007 d2, through the FORCE commitment of security.md (commitments C6)")
    with connect(owner_ci) as conn:
        tables = tenant_tables(conn)
        rls = conn.execute(
            "SELECT c.relname, c.relrowsecurity, c.relforcerowsecurity, "
            "       (SELECT count(*) FROM pg_policy p WHERE p.polrelid = c.oid), "
            "       pg_get_userbyid(c.relowner) "
            "  FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace "
            " WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') ORDER BY 1"
        ).fetchall()
        lacking = [r[0] for r in rls if not (r[1] and r[2] and r[3] > 0)]
        s.check(not lacking and len(rls) == len(tables),
                f"every one of the {len(rls)} tables has ENABLE and FORCE ROW LEVEL SECURITY and a policy"
                + (f"; lacking: {lacking}" if lacking else ""))
        owners = {r[4] for r in rls}
        s.check(owners == {owner_name}, f"the connecting role {owner_name} owns every table (owners: {sorted(owners)})")

        s.unprobed = unseeded(tables)
        for viewer, other in ((a, b), (b, a)):
            c = counts(conn, tables, viewer.id, other.id)
            foreign = {t: v[1] for t, v in c.items() if v[1]}
            empty = [t for t, v in c.items() if v[0] == 0 and t in SEEDED_TABLES]
            s.check(not foreign and not empty,
                    f"as owner with tenant {viewer.letter} set, every seeded table shows its own rows only "
                    f"({sum(v[0] for v in c.values())} rows over {len(c) - len(s.unprobed)} seeded tables, "
                    f"0 of tenant {other.letter})"
                    + (f"; foreign rows: {foreign}" if foreign else "") + (f"; empty: {empty}" if empty else ""))
        c = counts(conn, tables, None, b.id)
        s.check(all(v[0] == 0 for v in c.values()),
                f"as owner with no tenant set, every table shows 0 rows ({sum(v[0] for v in c.values())} seen)")

        state = None
        try:
            with tenant_tx(conn, a.id):
                conn.execute("SET LOCAL row_security = off")
                conn.execute("SELECT count(*) FROM memories").fetchone()
        except psycopg.Error as e:
            state = f"{e.sqlstate} {e.diag.message_primary}"
        s.check(state is not None and state.startswith("42501"),
                f"as owner, SET row_security = off does not unfilter a FORCE table; the read raises {state}")
        stats = statistics_rows(conn, a.id)
        leaking = [r for r in stats if tainted(r[2], b.identifiers())]
        s.check(not leaking,
                key="S3.1/owner/pg_stats",
                text=f"as owner with tenant A set, pg_stats ({len(stats)} rows for the schema) carries no value of "
                "tenant B" + (f"; it does, in {[f'{r[0]}.{r[1]}' for r in leaking]}: "
                              f"{[excerpt(r[2], MARKER['B']) for r in leaking]}" if leaking else ""))

        # Control: lift FORCE on memories inside a transaction that rolls back.
        leaked = 0
        with contextlib.suppress(_Rollback):
            with conn.transaction():
                conn.execute("ALTER TABLE memories NO FORCE ROW LEVEL SECURITY")
                set_tenant(conn, a.id)
                leaked = conn.execute(
                    "SELECT count(*) FROM memories WHERE tenant_id = %s", [b.id]).fetchone()[0]
                raise _Rollback
        forced = conn.execute(
            "SELECT relforcerowsecurity FROM pg_class WHERE oid = 'memories'::regclass").fetchone()[0]
        s.control(leaked > 0 and forced,
                  f"control: with FORCE lifted on memories, the owner set to tenant A reads {leaked} "
                  f"tenant B rows; the ALTER rolled back and FORCE is {'set' if forced else 'NOT set'} again")
        s.observations.append(
            "the owner can lift FORCE with ALTER TABLE ... NO FORCE ROW LEVEL SECURITY (shown by the "
            "control): FORCE binds an owner's reads, not an owner's DDL, which is why the querying role "
            "must not be the owner (S3.2)")
    return s


class _Rollback(Exception):
    pass


# ---------------------------------------------------------------------------
# S3.2 — owner/migration split; no application role holds BYPASSRLS
# ---------------------------------------------------------------------------


def s3_2(admin_db_ci: str, owner_ci: str, service_ci: str, owner: str, service: str,
         a: Tenant, b: Tenant, migration_log: list[tuple[str, bool, str]],
         backfill: list[tuple[bool, str]], control_role: str) -> Scenario:
    s = Scenario("S3.2", "Owner/migration-role split; application roles carry no BYPASSRLS",
                 "ADR-0007 d2 (no BYPASSRLS role), through the role split of security.md "
                 "(commitments C6; C3 for the journal re-grant)")
    s.settings.append(
        f"roles: owner {owner} and service {service}, both LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE "
        "NOBYPASSRLS; the database is owned by the owner role; after migrating, the owner grants the "
        "service role SELECT, INSERT, UPDATE, DELETE on every table but journal (SELECT, INSERT) and "
        "USAGE, SELECT on every sequence")
    s.check(all(ok for _, ok, _ in migration_log),
            "migrations succeed under the owner role: "
            + ", ".join(f"{name} {'ok' if ok else 'FAILED: ' + err}" for name, ok, err in migration_log))
    for ok, text in backfill:
        s.check(ok, text)

    with connect(admin_db_ci) as admin:
        attrs = admin.execute(
            "SELECT rolname, rolsuper, rolbypassrls, rolcreaterole, rolcreatedb FROM pg_roles "
            "WHERE rolname = ANY(%s::name[]) ORDER BY rolname", [[owner, service]]).fetchall()
        s.check(len(attrs) == 2 and not any(r[1] or r[2] or r[3] or r[4] for r in attrs),
                "neither the owner nor the service role is a superuser or holds BYPASSRLS, "
                f"CREATEROLE or CREATEDB: {attrs}")
        owned = admin.execute(
            "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace "
            "WHERE n.nspname = 'public' AND c.relowner = %s::regrole", [service]).fetchone()[0]
        s.check(owned == 0, f"the service role owns {owned} relations in the schema")
        member = admin.execute("SELECT pg_has_role(%s::name, %s::name, 'MEMBER')", [service, owner]).fetchone()[0]
        s.check(not member, f"the service role is {'a' if member else 'not a'} member of the owner role")
        bypassers = admin.execute(
            "SELECT rolname FROM pg_roles WHERE (rolbypassrls OR rolsuper) AND rolname NOT LIKE 'pg\\_%' "
            "ORDER BY 1").fetchall()
        s.observations.append(
            "roles in the cluster that bypass RLS (superuser or BYPASSRLS): "
            f"{[r[0] for r in bypassers]}; none of them is a role an OML workload uses here")

    with connect(service_ci) as conn:
        tables = tenant_tables(conn)
        s.unprobed = unseeded(tables)
        c = counts(conn, tables, a.id, b.id)
        foreign = {t: v[1] for t, v in c.items() if v[1]}
        empty = [t for t, v in c.items() if v[0] == 0 and t in SEEDED_TABLES]
        s.check(not foreign and not empty,
                f"as the service role with tenant A set, every seeded table shows tenant A rows only "
                f"({sum(v[0] for v in c.values())} rows, 0 of tenant B)" + (f"; foreign: {foreign}" if foreign else "")
                + (f"; empty: {empty}" if empty else ""))
        c = counts(conn, tables, None, b.id)
        s.check(all(v[0] == 0 for v in c.values()),
                "as the service role with no tenant set, every table shows 0 rows")
        refusals = {
            "ALTER TABLE memories NO FORCE ROW LEVEL SECURITY": "42501",
            "ALTER TABLE memories DISABLE ROW LEVEL SECURITY": "42501",
            "DROP POLICY tenant_isolation ON memories": "42501",
            "CREATE POLICY spike_open ON memories USING (true)": "42501",
            "ALTER TABLE memories OWNER TO CURRENT_USER": "42501",
            "UPDATE journal SET principal = principal": "42501",
            "DELETE FROM journal": "42501",
            "TRUNCATE journal": "42501",
        }
        for statement, expected in refusals.items():
            got = sqlstate_of(conn, statement)
            s.check(got == expected, f"as the service role, {statement} is refused with {got} (expected {expected})")
        got = sqlstate_of(conn, sql.SQL("SET ROLE {}").format(sql.Identifier(owner)))
        s.check(got == "42501", f"as the service role, SET ROLE to the owner is refused with {got}")
        state = None
        try:
            with tenant_tx(conn, a.id):
                conn.execute("SET LOCAL row_security = off")
                conn.execute("SELECT count(*) FROM memories").fetchone()
        except psycopg.Error as e:
            state = f"{e.sqlstate} {e.diag.message_primary}"
        s.check(state is not None and state.startswith("42501"),
                f"as the service role, SET row_security = off raises {state} rather than returning unfiltered rows")
        notices: list[str] = []
        conn.add_notice_handler(lambda d: notices.append(f"{d.severity}: {d.message_primary}"))
        grant = sqlstate_of(conn, sql.SQL("GRANT UPDATE ON journal TO {}").format(sql.Identifier(service)))
        can = conn.execute("SELECT has_table_privilege(current_user, 'journal', 'UPDATE')").fetchone()[0]
        s.check(not can, f"the service role cannot grant itself UPDATE on journal (GRANT gave "
                         f"{grant or 'no error'}, server said {notices or 'nothing'}; UPDATE privilege now {can})")

    # Control: a role with BYPASSRLS reads across the policy.
    with connect(admin_db_ci) as admin:
        with contextlib.suppress(_Rollback):
            with admin.transaction():
                admin.execute(sql.SQL("SET LOCAL ROLE {}").format(sql.Identifier(control_role)))
                set_tenant(admin, a.id)
                leaked = admin.execute("SELECT count(*) FROM memories WHERE tenant_id = %s", [b.id]).fetchone()[0]
                raise _Rollback
        s.control(leaked > 0, f"control: a NOLOGIN BYPASSRLS role with SELECT on memories, set to tenant A, "
                              f"reads {leaked} tenant B rows")
    with connect(owner_ci) as conn:
        upd = sqlstate_of(conn, "UPDATE journal SET principal = principal")
        s.observations.append(
            f"as the owner, UPDATE journal raises {upd}: the guard trigger is the fence that binds the owner "
            "(ADR-0002 d4); the owner could drop that trigger, which is the C3 reason the owner role is "
            "used by nothing but migrations")
    return s


# ---------------------------------------------------------------------------
# S3.3 — SET LOCAL discipline across pooled transactions
# ---------------------------------------------------------------------------


def s3_3(roles: dict[str, str], a: Tenant, b: Tenant) -> Scenario:
    s = Scenario("S3.3", "SET LOCAL tenant discipline across pooled transactions",
                 "ADR-0007 d3, through the SET LOCAL commitment of security.md (commitments C6); "
                 "ADR-0024 d4 for prepared statements on a pooled connection")
    s.settings.append("psycopg_pool.ConnectionPool(min_size=1, max_size=1, autocommit=True): one "
                      "physical connection serves every checkout")
    for role_label, ci in roles.items():
        s3_3_for(s, role_label, ci, a, b)
    return s


def setting(conn: psycopg.Connection) -> str | None:
    return conn.execute("SELECT current_setting('oml.tenant_id', true)").fetchone()[0]


def memory_tenants(conn: psycopg.Connection) -> list[uuid.UUID]:
    return [r[0] for r in conn.execute("SELECT tenant_id FROM memories").fetchall()]


def s3_3_for(s: Scenario, role: str, ci: str, a: Tenant, b: Tenant) -> None:
    pool = ConnectionPool(ci, min_size=1, max_size=1, kwargs={"autocommit": True}, open=True)
    try:
        pids: set[int] = set()
        with pool.connection() as conn:
            pids.add(conn.info.backend_pid)
            tables = tenant_tables(conn)
            fresh = setting(conn)
            c = counts(conn, tables, None, b.id)
            s.check(fresh is None and all(v[0] == 0 for v in c.values()),
                    f"[{role}] on a fresh pooled connection oml.tenant_id is {fresh!r} and every table shows 0 rows")

        sequence = []
        for i in range(8):
            t = a if i % 2 == 0 else b
            end = "commit" if i % 4 < 2 else "rollback"
            form = "set_config" if i % 3 else "SET LOCAL"
            with pool.connection() as conn:
                pids.add(conn.info.backend_pid)
                before = setting(conn)
                conn.execute("BEGIN")
                if form == "set_config":
                    set_tenant(conn, t.id)
                else:
                    conn.execute(sql.SQL("SET LOCAL oml.tenant_id = {}").format(sql.Literal(str(t.id))))
                seen = memory_tenants(conn)
                conn.execute("COMMIT" if end == "commit" else "ROLLBACK")
                after = setting(conn)
                left = len(memory_tenants(conn))
            ok = before in (None, "") and seen and all(x == t.id for x in seen) and after == "" and left == 0
            sequence.append(ok)
            if not ok:
                s.check(False, f"[{role}] transaction {i} (tenant {t.letter}, {form}, {end}): setting before "
                               f"{before!r}, saw tenants {sorted({str(x)[:8] for x in seen})}, after {after!r}, "
                               f"{left} rows after")
        s.check(all(sequence), f"[{role}] 8 interleaved transactions A,B,A,B,... on one connection, ended by "
                               "commit and by rollback, set by set_config(..., true) and by SET LOCAL: each saw "
                               "only its own tenant's memories, and after each the setting was '' and 0 rows showed")

        with pool.connection() as conn:
            pids.add(conn.info.backend_pid)
            try:
                with tenant_tx(conn, a.id):
                    conn.execute("SELECT 1/0")
            except errors.DivisionByZero:
                pass
            s.check(setting(conn) == "" and not memory_tenants(conn),
                    f"[{role}] after a transaction aborted by an error, the setting is '' and 0 rows show")

            with conn.transaction():
                set_tenant(conn, a.id)
                try:
                    with conn.transaction():  # SAVEPOINT
                        conn.execute(sql.SQL("SET LOCAL oml.tenant_id = {}").format(sql.Literal(str(b.id))))
                        inner = setting(conn)
                        raise _Rollback
                except _Rollback:
                    pass
                restored = setting(conn)
                seen = memory_tenants(conn)
            s.check(inner == str(b.id) and restored == str(a.id) and seen and all(x == a.id for x in seen)
                    and setting(conn) == "",
                    f"[{role}] SET LOCAL inside a savepoint that rolls back reverts to the outer tenant "
                    f"(inner {inner == str(b.id)}, restored {restored == str(a.id)}), and the outer commit clears it")

            # Prepared statements reused across tenants (ADR-0024 d4).
            q = "SELECT tenant_id FROM memories WHERE status = %s"
            results = []
            for t in (a, b, None, a, b, None):
                with tenant_tx(conn, t.id if t else None):
                    results.append((t, [r[0] for r in conn.execute(q, ["promoted"], prepare=True).fetchall()]))
            ok = all((not seen) if t is None else (seen and all(x == t.id for x in seen)) for t, seen in results)
            s.check(ok, f"[{role}] one protocol-level prepared statement executed under A, B, none, A, B, none "
                        "returns each tenant's rows only, and none with no tenant")
            conn.execute("SET plan_cache_mode = force_generic_plan")
            conn.execute("PREPARE spike_q(text) AS SELECT tenant_id FROM memories WHERE claim_key = $1")
            generic = []
            for t in (a, b) * 4:
                with tenant_tx(conn, t.id):
                    seen = [r[0] for r in conn.execute("EXECUTE spike_q('deploy-runbook')").fetchall()]
                    generic.append(bool(seen) and all(x == t.id for x in seen))
            conn.execute("DEALLOCATE spike_q")
            conn.execute("RESET plan_cache_mode")
            s.check(all(generic), f"[{role}] a SQL PREPARE under plan_cache_mode = force_generic_plan, executed "
                                  "8 times alternating A and B, returns each tenant's rows only")

            notices: list[str] = []

            def handler(d) -> None:
                notices.append(f"{d.severity} {d.sqlstate}: {d.message_primary}")

            conn.add_notice_handler(handler)
            conn.execute(sql.SQL("SET LOCAL oml.tenant_id = {}").format(sql.Literal(str(a.id))))
            outside = memory_tenants(conn)
            conn.remove_notice_handler(handler)
            s.check(not outside and any("25P01" in n for n in notices),
                    f"[{role}] SET LOCAL outside a transaction block has no effect: the server warns "
                    f"({notices[-1] if notices else 'no warning'}) and the next statement shows 0 rows")

            # A row every constraint and domain of the schema accepts (a lowercase label, under
            # A's universe), so the policy is the only thing that can refuse it.
            state = sqlstate_of(conn, "INSERT INTO workareas (tenant_id, parent_id, name, status) "
                                      "VALUES (%s, %s, 'no-tenant-write', 'development')", [a.id, a.universe])
            s.check(state == "42501", f"[{role}] a write with no tenant set is refused with {state} "
                                      "(new row violates row-level security policy)")

        # A connection returned to the pool inside an open transaction.
        conn = pool.getconn()
        pids.add(conn.info.backend_pid)
        conn.execute("BEGIN")
        set_tenant(conn, a.id)
        pool.putconn(conn)
        with pool.connection() as conn:
            pids.add(conn.info.backend_pid)
            s.check(setting(conn) == "" and not memory_tenants(conn),
                    f"[{role}] a connection returned to the pool inside an open transaction comes back rolled "
                    "back: the setting is '' and 0 rows show")
        s.check(len(pids) == 1, f"[{role}] every checkout above used one backend (pids {sorted(pids)})")

        # Control: session-level SET survives commit.
        with pool.connection() as conn:
            conn.execute(sql.SQL("SET oml.tenant_id = {}").format(sql.Literal(str(a.id))))
        with pool.connection() as conn:
            with conn.transaction():
                survived = memory_tenants(conn)
            conn.execute("RESET oml.tenant_id")
            reset = setting(conn)
        s.control(bool(survived), f"control [{role}]: a session-level SET oml.tenant_id survives into the next "
                                  f"pooled checkout and its transaction reads {len(survived)} tenant A rows; "
                                  f"RESET leaves {reset!r}")
    finally:
        pool.close()

    # The schema page's sketch form, without missing_ok and NULLIF.
    with connect(ci) as conn:
        fresh = sqlstate_of(conn, "SELECT current_setting('oml.tenant_id')::uuid")
        with conn.transaction():
            set_tenant(conn, a.id)
        after = sqlstate_of(conn, "SELECT current_setting('oml.tenant_id')::uuid")
    s.observations.append(
        f"[{role}] the postgres-schema.md sketch's predicate form, current_setting('oml.tenant_id')::uuid, "
        f"raises {fresh} on a connection that never set the tenant and {after} on one that set and ended "
        "it; the migrations' NULLIF(current_setting('oml.tenant_id', true), '')::uuid yields NULL in both "
        "states, so a read with no tenant returns 0 rows")


# ---------------------------------------------------------------------------
# S3.4 — hostile cross-tenant probe over every read shape
# ---------------------------------------------------------------------------


def b_shapes(a: Tenant, b: Tenant) -> tuple[list[tuple[str, str, Any, str]], list[tuple[str, str, Any, str]]]:
    """The adapter's read shapes (02a385d), each with tenant B's values in its parameters.

    (name, sql, params, kind): kind "rows" must return no rows under tenant A; kind
    "aggregate" must return the aggregate of no rows; kind "cast" must return no rows and
    no error under tenant A, while under tenant B its cast fails on B's text.
    """
    words = "runbook drain"
    adapter = [
        ("chain head", "SELECT seq, head_hash FROM journal_chain_head WHERE tenant_id = %s", [b.id], "rows"),
        ("journal range", "SELECT signed_bytes, signed_bytes_hash, sig FROM journal WHERE tenant_id = %s "
                          "AND seq >= %s ORDER BY seq ASC LIMIT %s", [b.id, 1, 100], "rows"),
        ("tail-scan subjects", "SELECT id, workarea_id, author_key_id FROM memories WHERE tenant_id = %s "
                               "AND id = ANY(%s::uuid[])", [b.id, b.memories], "rows"),
        ("get memory", f"SELECT {MEMORY_COLUMNS} FROM memories m WHERE m.tenant_id = %s AND m.id = %s",
         [b.id, b.m1], "rows"),
        ("hydrate", f"SELECT {MEMORY_COLUMNS} FROM memories m WHERE m.tenant_id = %s AND m.id = ANY(%s::uuid[]) "
                    "ORDER BY array_position(%s::uuid[], m.id)", [b.id, b.memories, b.memories], "rows"),
        ("recall FTS", "SELECT m.id, m.body_bytes, ts_rank(to_tsvector('simple', m.claim), "
                       "plainto_tsquery('simple', %s), 0) AS score FROM memories m WHERE m.tenant_id = %s "
                       "AND m.status = 'promoted' AND m.is_tip AND NOT m.retirement AND m.kind = ANY(%s::text[]) "
                       "AND m.workarea_id = ANY(%s::uuid[]) AND to_tsvector('simple', m.claim) @@ "
                       "plainto_tsquery('simple', %s) ORDER BY score DESC, m.id ASC LIMIT %s",
         [words, b.id, ["memory", "steering"], b.nodes, words, 10], "rows"),
        ("recall own held FTS", "SELECT m.id, m.body_bytes, ts_rank(to_tsvector('simple', m.claim), "
                                "plainto_tsquery('simple', %(q)s), 0) AS score FROM memories m WHERE m.tenant_id = "
                                "%(t)s AND m.status = 'held' AND m.author_key_id = ANY(%(a)s::uuid[]) AND NOT "
                                "m.retirement AND m.kind = ANY(%(k)s::text[]) AND m.workarea_id = ANY(%(w)s::uuid[]) "
                                "AND to_tsvector('simple', m.claim) @@ plainto_tsquery('simple', %(q)s) ORDER BY "
                                "score DESC, m.id ASC LIMIT %(n)s",
         {"q": words, "t": b.id, "a": [b.writer], "k": ["memory"], "w": b.nodes, "n": 10}, "rows"),
        ("steering", f"SELECT {MEMORY_COLUMNS} FROM memories m WHERE m.tenant_id = %s AND m.status = 'promoted' "
                     "AND m.is_tip AND NOT m.retirement AND m.kind = 'steering' AND m.workarea_id = ANY(%s::uuid[]) "
                     "ORDER BY array_position(%s::uuid[], m.workarea_id) ASC, m.promoted_at ASC, m.id ASC",
         [b.id, b.nodes, b.nodes], "rows"),
        ("own listing", f"SELECT {MEMORY_COLUMNS} FROM memories m WHERE m.tenant_id = %s AND m.author_key_id = "
                        "ANY(%s::uuid[]) AND NOT (m.workarea_id = ANY(%s::uuid[])) ORDER BY m.created_at DESC, "
                        "m.id DESC LIMIT %s", [b.id, [b.writer], [], 50], "rows"),
        ("tips listing, every filter", f"SELECT {MEMORY_COLUMNS} FROM memories m WHERE m.tenant_id = %s AND "
                                       "m.status = 'promoted' AND m.is_tip AND m.workarea_id = ANY(%s::uuid[]) AND "
                                       "m.workarea_id = ANY(%s::uuid[]) AND m.status = %s AND m.kind = %s AND "
                                       "m.claim_key = %s AND m.created_at >= (%s)::timestamptz AND m.created_at < "
                                       "(%s)::timestamptz AND (m.created_at, m.id) < ((%s)::timestamptz, %s) "
                                       "ORDER BY m.created_at DESC, m.id DESC LIMIT %s",
         [b.id, b.nodes, b.nodes, "promoted", "memory", "deploy-runbook", "2000-01-01T00:00:00Z",
          "2999-01-01T00:00:00Z", "2999-01-01T00:00:00Z", uuid.UUID(int=2**128 - 1), 50], "rows"),
        ("claim_key state", "SELECT max(version) AS greatest, coalesce(bool_or(status IN ('held', 'promoted', "
                            "'demoted')), false) AS live FROM memories WHERE tenant_id = %s AND workarea_id = %s "
                            "AND claim_key = %s", [b.id, b.child, "deploy-runbook"], "aggregate"),
        ("served tip", f"SELECT {MEMORY_COLUMNS} FROM memories m WHERE m.tenant_id = %s AND m.workarea_id = %s "
                       "AND m.claim_key = %s AND m.is_tip AND m.status = 'promoted'",
         [b.id, b.child, "deploy-runbook"], "rows"),
        ("held successors", f"SELECT {MEMORY_COLUMNS} FROM memories m WHERE m.tenant_id = %s AND "
                            "m.replaces_artifact_id = %s AND m.status = 'held' ORDER BY m.created_at ASC, m.id ASC",
         [b.id, b.m1], "rows"),
        ("review claim next", f"SELECT {MEMORY_COLUMNS} FROM review_queue q JOIN memories m ON m.id = q.memory_id "
                              "WHERE q.tenant_id = %(t)s AND m.workarea_id = ANY(%(w)s::uuid[]) AND (q.held_by_key "
                              "IS NULL OR q.held_by_key = %(k)s) AND NOT (m.lineage_principals @> ARRAY[%(p)s]::text[]) "
                              "AND NOT (m.lineage_key_ids @> ARRAY[%(k)s]::uuid[]) AND NOT EXISTS (SELECT 1 FROM "
                              "memories r WHERE r.tenant_id = %(t)s AND r.id <> m.id AND r.status = 'held' AND "
                              "r.replaces_artifact_id = m.replaces_artifact_id AND r.workarea_id = m.workarea_id AND "
                              "r.claim_key = m.claim_key AND (r.author_key_id = %(k)s OR r.author_principal = %(p)s)) "
                              "ORDER BY q.held_since ASC, q.memory_id ASC LIMIT 1 FOR NO KEY UPDATE OF q SKIP LOCKED",
         {"t": b.id, "w": b.nodes, "k": a.dreamer, "p": f"{a.marker}/dreamer-1"}, "rows"),
        ("key by hash", "SELECT id, tenant_id, role, principal, held_by, default_workarea_id, issued_by, hash "
                        "FROM api_keys WHERE hash = %s AND revoked_at IS NULL", [key_hash("B", "writer")], "rows"),
        ("key issued_seq", "SELECT issued_seq FROM api_keys WHERE tenant_id = %s AND id = %s", [b.id, b.writer], "rows"),
        ("grants of a key", "SELECT id, role, workarea_node_id, deny, epoch FROM grants WHERE tenant_id = %s AND "
                            "key_id = %s ORDER BY epoch ASC, id ASC", [b.id, b.writer], "rows"),
        ("denials of a key", "SELECT id, workarea_node_id, created_by, epoch FROM key_denials WHERE tenant_id = %s "
                             "AND key_id = %s ORDER BY epoch ASC, id ASC", [b.id, b.writer], "rows"),
        ("workarea get", f"SELECT {WORKAREA_COLUMNS} FROM workareas WHERE tenant_id = %s AND id = %s",
         [b.id, b.child], "rows"),
        ("workarea ancestry", f"WITH RECURSIVE up AS (SELECT {WORKAREA_COLUMNS}, 0 AS depth FROM workareas WHERE "
                              "tenant_id = %(t)s AND id = %(n)s UNION ALL SELECT w.id, w.tenant_id, w.parent_id, "
                              "w.name, w.crew, up.depth + 1 FROM workareas w JOIN up ON w.id = up.parent_id WHERE "
                              f"w.tenant_id = %(t)s) SELECT {WORKAREA_COLUMNS} FROM up ORDER BY depth ASC",
         {"t": b.id, "n": b.child}, "rows"),
        ("workarea subtree", f"WITH RECURSIVE down AS (SELECT {WORKAREA_COLUMNS}, 0 AS depth FROM workareas WHERE "
                             "tenant_id = %(t)s AND id = %(n)s UNION ALL SELECT w.id, w.tenant_id, w.parent_id, "
                             "w.name, w.crew, down.depth + 1 FROM workareas w JOIN down ON w.parent_id = down.id "
                             f"WHERE w.tenant_id = %(t)s) SELECT {WORKAREA_COLUMNS} FROM down ORDER BY depth ASC, id ASC",
         {"t": b.id, "n": b.universe}, "rows"),
        ("workarea children", f"SELECT {WORKAREA_COLUMNS} FROM workareas WHERE tenant_id = %s AND parent_id = %s "
                              "ORDER BY name COLLATE \"C\" ASC, id ASC", [b.id, b.universe], "rows"),
        ("idempotency get", "SELECT request_digest, response FROM idempotency WHERE tenant_id = %s AND key = %s",
         [b.id, b.idem_key], "rows"),
        ("review claim update", "UPDATE review_queue SET held_by_key = %s WHERE tenant_id = %s AND memory_id = %s "
                                "RETURNING *", [a.dreamer, b.id, b.m2], "rows"),
    ]
    beyond = [
        ("join by B ids", "SELECT m.* FROM (VALUES (%s::uuid), (%s::uuid)) v(id) JOIN memories m ON m.id = v.id",
         [b.m1, b.m2], "rows"),
        ("IN subquery", "SELECT * FROM workareas WHERE id IN (SELECT workarea_id FROM memories WHERE tenant_id = %s)",
         [b.id], "rows"),
        ("EXISTS", "SELECT EXISTS (SELECT 1 FROM memories WHERE id = %s)", [b.m1], "aggregate"),
        ("count by tenant", "SELECT count(*) FROM memories WHERE tenant_id = %s", [b.id], "aggregate"),
        ("jsonb payload ->>", "SELECT * FROM journal WHERE payload ->> 'artifact_id' = %s", [str(b.m1)], "rows"),
        ("jsonb payload @>", "SELECT * FROM journal WHERE payload @> %s::jsonb",
         [json.dumps({"key_id": str(b.writer)})], "rows"),
        ("array containment", "SELECT id FROM memories WHERE lineage_principals @> ARRAY[%s]::text[]",
         [b.principal], "rows"),
        ("FTS on B's words", "SELECT id, ts_rank(to_tsvector('simple', claim), plainto_tsquery('simple', %s)) "
                             "FROM memories WHERE to_tsvector('simple', claim) @@ plainto_tsquery('simple', %s) "
                             "ORDER BY 2 DESC LIMIT 10", ["secret", "secret"], "rows"),
        ("locking read FOR UPDATE", "SELECT * FROM memories WHERE tenant_id = %s FOR UPDATE", [b.id], "rows"),
        ("locking read FOR SHARE", "SELECT * FROM api_keys WHERE tenant_id = %s FOR SHARE", [b.id], "rows"),
        ("UPDATE ... RETURNING", "UPDATE memories SET claim = claim WHERE tenant_id = %s RETURNING *", [b.id], "rows"),
        ("UPDATE ... FROM (tip move)", "UPDATE memories t SET is_tip = false FROM memories s WHERE t.tenant_id = %(t)s "
                                       "AND s.tenant_id = %(t)s AND s.id = %(s)s AND t.id = s.replaces_artifact_id "
                                       "RETURNING t.id", {"t": b.id, "s": b.m4}, "rows"),
        ("DELETE ... RETURNING", "DELETE FROM review_queue WHERE tenant_id = %s AND memory_id = %s RETURNING *",
         [b.id, b.m2], "rows"),
        ("cast error", "SELECT id FROM memories WHERE tenant_id = %s AND claim::int > 0", [b.id], "cast"),
    ]
    return adapter, beyond


def generic_shapes(tables: dict[str, str]) -> list[tuple[str, str, list]]:
    """Shapes with no tenant predicate: tenant A rows may return, tenant B's must not."""
    shapes = []
    for table, col in tables.items():
        shapes.append((f"full scan {table}", f'SELECT * FROM "{table}"', []))
        shapes.append((f"group by tenant {table}", f'SELECT "{col}", count(*) FROM "{table}" GROUP BY 1', []))
    shapes += [
        ("join memories-workareas", "SELECT m.id, w.name, m.claim FROM memories m JOIN workareas w ON w.id = m.workarea_id", []),
        ("join grants-api_keys", "SELECT g.id, k.principal FROM grants g JOIN api_keys k ON k.id = g.key_id", []),
        ("join journal-outbox", "SELECT j.event_id, o.payload FROM journal j JOIN outbox o ON o.tenant_id = j.tenant_id "
                                "AND o.tenant_seq = j.seq", []),
        ("join memories-journal", "SELECT m.id, j.payload FROM memories m JOIN journal j ON j.event_id = m.created_event_id", []),
        ("join review_queue-memories", "SELECT q.memory_id, m.claim FROM review_queue q JOIN memories m ON m.id = q.memory_id", []),
        ("LATERAL", "SELECT w.name, x.claim FROM workareas w, LATERAL (SELECT claim FROM memories m "
                    "WHERE m.workarea_id = w.id ORDER BY created_at LIMIT 2) x", []),
        ("UNION ALL", "SELECT tenant_id::text FROM memories UNION ALL SELECT tenant_id::text FROM api_keys "
                      "UNION ALL SELECT id::text FROM tenants", []),
        ("window", "SELECT tenant_id, id, row_number() OVER (PARTITION BY tenant_id ORDER BY created_at) FROM memories", []),
        ("ORDER BY LIMIT", "SELECT id, claim FROM memories ORDER BY created_at DESC LIMIT 100", []),
        ("terms overlap", "SELECT id FROM memories WHERE terms && ARRAY['oml:runbook']", []),
        ("leakproof-free predicate", "SELECT id FROM memories WHERE claim ~ 'SECRET'", []),
    ]
    return shapes


BTREE = ["enable_seqscan = off", "enable_bitmapscan = off"]
BITMAP = ["enable_seqscan = off", "enable_indexscan = off"]


def index_shapes(b: Tenant) -> list[tuple[str, str, list, list[str]]]:
    """One probe per index of the schema, naming tenant B's values, with the planner
    settings that make its index the cheapest path (a GIN index has only bitmap scans)."""
    return [
        ("memories_workarea", "SELECT * FROM memories WHERE tenant_id = %s AND workarea_id = %s AND status = 'held'",
         [b.id, b.child], BTREE),
        ("memories_claim_fts", "SELECT * FROM memories WHERE to_tsvector('simple', claim) @@ "
                               "plainto_tsquery('simple', 'secret')", [], BITMAP),
        ("memories_author", "SELECT * FROM memories WHERE tenant_id = %s AND author_key_id = %s "
                            "ORDER BY created_at DESC", [b.id, b.writer], BTREE),
        ("memories_served_tip", "SELECT * FROM memories WHERE tenant_id = %s AND workarea_id = %s AND "
                                "claim_key = 'deploy-runbook' AND is_tip AND status = 'promoted'", [b.id, b.child], BTREE),
        ("memories_claim_version", "SELECT * FROM memories WHERE tenant_id = %s AND workarea_id = %s AND "
                                   "claim_key = 'deploy-runbook' AND version = 2 AND status = 'held'", [b.id, b.child], BTREE),
        ("memories_pkey", "SELECT * FROM memories WHERE id = %s", [b.m1], BTREE),
        ("grants_key", "SELECT * FROM grants WHERE tenant_id = %s AND key_id = %s", [b.id, b.writer], BTREE),
        ("grants_pkey", "SELECT * FROM grants WHERE id = %s", [b.grant_w], BTREE),
        ("key_denials_key", "SELECT * FROM key_denials WHERE tenant_id = %s AND key_id = %s", [b.id, b.writer], BTREE),
        ("key_denials_pkey", "SELECT * FROM key_denials WHERE id = %s", [b.denial], BTREE),
        ("api_keys_hash_key", "SELECT * FROM api_keys WHERE hash = %s", [key_hash("B", "writer")], BTREE),
        ("api_keys_pkey", "SELECT * FROM api_keys WHERE id = %s", [b.writer], BTREE),
        ("api_keys_tenant_id_id", "SELECT tenant_id, id FROM api_keys WHERE tenant_id = %s AND id > %s",
         [b.id, uuid.UUID(int=0)], BTREE),
        ("journal_event_id_key", "SELECT * FROM journal WHERE event_id = %s", [b.events[4]], BTREE),
        ("journal_pkey", "SELECT * FROM journal WHERE tenant_id = %s AND seq BETWEEN 1 AND 9", [b.id], BTREE),
        ("journal_chain_head_pkey", "SELECT * FROM journal_chain_head WHERE tenant_id = %s", [b.id], BTREE),
        ("outbox_pkey", "SELECT * FROM outbox WHERE seq BETWEEN 1 AND 1000", [], BTREE),
        ("review_queue_pkey", "SELECT * FROM review_queue WHERE memory_id = %s", [b.m2], BTREE),
        ("idempotency_pkey", "SELECT * FROM idempotency WHERE tenant_id = %s AND key = %s", [b.id, b.idem_key], BTREE),
        ("tenants_pkey", "SELECT * FROM tenants WHERE id = %s", [b.id], BTREE),
        ("workareas_pkey", "SELECT * FROM workareas WHERE id = %s", [b.child], BTREE),
        ("workareas_tenant_id_id", "SELECT tenant_id, id FROM workareas WHERE tenant_id = %s AND id > %s",
         [b.id, uuid.UUID(int=0)], BTREE),
        ("workareas_universe", "SELECT * FROM workareas WHERE tenant_id = %s AND parent_id IS NULL", [b.id], BTREE),
    ]


FTS_PROBE = ("SELECT id FROM memories WHERE to_tsvector('simple', claim) @@ plainto_tsquery('simple', 'secret')")


def gin_under_rls(conn: psycopg.Connection, tenant: uuid.UUID, owner: bool) -> str:
    """Whether an FTS predicate can use the GIN index while RLS binds the session."""
    leakproof = dict(conn.execute(
        "SELECT o.oprname || ' (' || p.proname || ')', p.proleakproof FROM pg_operator o "
        "JOIN pg_proc p ON p.oid = o.oprcode WHERE (o.oprname = '@@' AND o.oprleft = 'tsvector'::regtype) "
        "OR (o.oprname IN ('@>', '&&') AND o.oprleft = 'anyarray'::regtype) "
        "OR (o.oprname IN ('@>', '->>') AND o.oprleft = 'jsonb'::regtype AND o.oprright IN ('jsonb'::regtype, 'text'::regtype)) "
        "OR (o.oprname = '=' AND o.oprleft = 'uuid'::regtype AND o.oprright = 'uuid'::regtype) "
        "ORDER BY 1").fetchall())
    plans = {}
    for lifted in ((False, True) if owner else (False,)):
        try:
            with conn.transaction():
                if lifted:
                    conn.execute("ALTER TABLE memories NO FORCE ROW LEVEL SECURITY")
                set_tenant(conn, tenant)
                conn.execute("SET LOCAL enable_seqscan = off")
                plans[lifted] = plan_indexes(conn.execute(f"EXPLAIN (FORMAT JSON) {FTS_PROBE}").fetchone()[0])
                raise _Rollback
        except _Rollback:
            pass
    text = (f"leakproof flags: {leakproof}. With RLS binding the session, an FTS predicate's plan uses "
            f"{sorted(plans[False]) or 'no index'}")
    if True in plans:
        text += (f"; with FORCE lifted for the owner (rolled back) the same plan uses {sorted(plans[True]) or 'no index'}")
    return text + (". A predicate whose operator is not leakproof cannot become an index condition beneath "
                   "an RLS policy, so the memories_claim_fts GIN index serves no RLS-bound query")


def plan_indexes(plan: Any) -> set[str]:
    found: set[str] = set()
    if isinstance(plan, dict):
        if "Index Name" in plan:
            found.add(plan["Index Name"])
        for v in plan.values():
            found |= plan_indexes(v)
    elif isinstance(plan, list):
        for v in plan:
            found |= plan_indexes(v)
    return found


def run_rows(conn, statement, params) -> tuple[list[tuple], str | None]:
    try:
        with conn.transaction():
            cur = conn.execute(statement, params)
            rows = cur.fetchall() if cur.description else []
            raise _Rollback  # writes in a probe never commit
    except _Rollback:
        return rows, None
    except psycopg.Error as e:
        return [], f"{e.sqlstate} {e.diag.message_primary}"


def s3_4(roles: dict[str, str], a: Tenant, b: Tenant) -> Scenario:
    s = Scenario("S3.4", "Hostile cross-tenant probe over every read shape",
                 "ADR-0007 d2 and d6 (RLS as the second fence, judged against the hostile co-tenant)")
    s.settings.append("each probe runs in a transaction with oml.tenant_id = tenant A, set as the adapter "
                      "sets it, and rolls back; tenant B's values are in the parameters (the WHERE-clause-bug case)")
    needles = b.identifiers() | {str(b.id)}
    for role, ci in roles.items():
        s3_4_for(s, role, ci, a, b, needles)
    return s


def s3_4_for(s: Scenario, role: str, ci: str, a: Tenant, b: Tenant, needles: set[str]) -> None:
    notices: list[str] = []
    with connect(ci, notices) as conn:
        tables = tenant_tables(conn)
        adapter, beyond = b_shapes(a, b)
        shapes = adapter + beyond

        def under(tenant: uuid.UUID, statement, params):
            with conn.transaction():
                set_tenant(conn, tenant)
                return run_rows(conn, statement, params)

        bad, insensitive, errored = [], [], []
        for name, statement, params, kind in shapes:
            rows, err = under(a.id, statement, params)
            if err:
                errored.append(f"{name}: {err}")
            if kind == "aggregate":
                if not rows or not all(v in (None, False, 0) for v in rows[0]):
                    bad.append(f"{name}: {rows}")
            elif rows or rows_tainted(rows, needles):
                bad.append(f"{name}: {len(rows)} rows")
            ctl, cerr = under(b.id, statement, params)
            if kind == "aggregate":
                hit = bool(ctl) and any(v not in (None, False, 0) for v in ctl[0])
            elif kind == "cast":
                hit = cerr is not None and MARKER["B"] in cerr
            else:
                hit = bool(ctl)
            if not hit:
                insensitive.append(f"{name}: {cerr or 'nothing found under tenant B'}")
        s.check(not bad, f"[{role}] {len(shapes)} read shapes that name tenant B's rows (the adapter's "
                         f"{len(adapter)} at 02a385d, and joins, subqueries, jsonb and array predicates, FTS, locking reads, "
                         "UPDATE/DELETE ... RETURNING, a cast that would error on B's text) return 0 rows under "
                         "tenant A" + (f"; returned: {bad}" if bad else ""))
        s.check(not errored, f"[{role}] none of those shapes errors under tenant A"
                             + (f"; errors: {errored}" if errored else ""))
        s.control(not insensitive, f"control [{role}]: the same {len(shapes)} shapes run with the tenant set to B "
                                   "each return B's rows, so each shape can find what it hunts"
                                   + (f"; insensitive: {insensitive}" if insensitive else ""))

        gen = generic_shapes(tables)
        leaks = []
        for name, statement, params in gen:
            rows, err = under(a.id, statement, params)
            n = rows_tainted(rows, needles)
            if n or err:
                leaks.append(f"{name}: {n} tainted rows {err or ''}")
        s.unprobed = unseeded(tables)
        s.check(not leaks, f"[{role}] {len(gen)} shapes with no tenant predicate (full scans and GROUP BY of all "
                           f"{len(tables)} tables, joins, LATERAL, UNION ALL, window, ORDER BY LIMIT, overlap, a "
                           "non-leakproof regex) return tenant A rows and no value of tenant B"
                           + (f"; {leaks}" if leaks else "")
                           + (f"; {s.unprobed} hold no seeded row, so their scans could show none"
                              if s.unprobed else ""))

        schema_indexes = {r[0] for r in conn.execute(
            "SELECT indexname FROM pg_indexes WHERE schemaname = 'public'").fetchall()}
        probes = index_shapes(b)
        missed, idx_bad = [], []
        for index, statement, params, settings in probes:
            with conn.transaction():
                set_tenant(conn, a.id)
                for setting_line in settings:
                    conn.execute(f"SET LOCAL {setting_line}")
                plan = conn.execute(f"EXPLAIN (FORMAT JSON) {statement}", params).fetchone()[0]
                rows = conn.execute(statement, params).fetchall()
            used = plan_indexes(plan)
            if index not in used:
                missed.append(f"{index} (plan used {sorted(used) or 'no index'})")
            if rows_tainted(rows, needles):
                idx_bad.append(f"{index}: {rows_tainted(rows, needles)} rows of tenant B")
        unprobed = sorted(schema_indexes - {p[0] for p in probes})
        s.check(not idx_bad, f"[{role}] {len(probes)} probes, one per index of the schema's {len(schema_indexes)}, "
                             "with seq scans (and, for btree probes, bitmap scans) disabled, return no row of tenant B; "
                             f"{len(probes) - len(missed)} of the plans used the index the probe names"
                             + (f"; {idx_bad}" if idx_bad else "") + (f"; not probed: {unprobed}" if unprobed else ""))
        if missed:
            s.observations.append(f"[{role}] index probes whose plan used another index: {missed}")
        s.observations.append(f"[{role}] " + gin_under_rls(conn, a.id, owner=role.startswith("owner")))

        # Cursor and COPY.
        with conn.transaction():
            set_tenant(conn, a.id)
            conn.execute("DECLARE spike_c CURSOR FOR SELECT * FROM memories")
            cursor_rows = conn.execute("FETCH ALL FROM spike_c").fetchall()
            copied = b""
            for statement in ("COPY memories TO STDOUT", "COPY (SELECT * FROM journal) TO STDOUT"):
                with conn.cursor().copy(statement) as copy:
                    for chunk in copy:
                        copied += bytes(chunk)
        s.check(not rows_tainted(cursor_rows, needles) and not tainted(copied, needles) and copied,
                f"[{role}] a cursor over memories and COPY memories / COPY (SELECT * FROM journal) TO STDOUT "
                f"carry no value of tenant B ({len(cursor_rows)} cursor rows, {len(copied)} COPY bytes)")

        # A non-leakproof function in the WHERE clause sees only rows that passed the policy.
        conn.execute(
            "CREATE FUNCTION pg_temp.spike_leak(text) RETURNS boolean LANGUAGE plpgsql COST 0.0001 AS "
            "$$ BEGIN RAISE NOTICE 'spike-saw:%', $1; RETURN true; END $$")
        # The function is handed the whole row as text, so a migration that renames or moves a
        # column cannot leave it handed NULLs.
        probes = [f"SELECT 1 FROM {t} x WHERE pg_temp.spike_leak(x::text)"
                  for t in ("memories", "workareas", "api_keys", "journal", "tenants", "outbox")]
        seen: dict[str, list[str]] = {}
        for tenant, label in ((a.id, "A"), (b.id, "B")):
            notices.clear()
            for p in probes:
                under(tenant, p, None)
            seen[label] = [n for n in notices if "spike-saw:" in n]
        seen_a, seen_b = seen["A"], seen["B"]
        s.check(seen_a and not any(tainted(n, needles) for n in seen_a),
                f"[{role}] a non-leakproof plpgsql function of cost 0.0001 in the WHERE clause of 6 tables, "
                f"under tenant A, was handed {len(seen_a)} values and none of tenant B")
        s.control(any(MARKER["B"] in n for n in seen_b),
                  f"control [{role}]: the same function under tenant B is handed B's values "
                  f"({sum(MARKER['B'] in n for n in seen_b)} of {len(seen_b)})")

        # Planner statistics.
        stats = statistics_rows(conn, a.id)
        leaking = [r for r in stats if tainted(r[2], needles)]
        s.check(not leaking,
                key=f"S3.4/{role.split()[0]}/pg_stats",
                text=f"[{role}] pg_stats shows {len(stats)} rows for the public schema to a session set to tenant A, "
                "none carrying a value of tenant B"
                + (f"; {[f'{r[0]}.{r[1]}' for r in leaking]} does: {[excerpt(r[2], MARKER['B']) for r in leaking]}"
                   if leaking else ""))

        # Observations: channels that are not row reads.
        obs = []
        reltuples = conn.execute("SELECT reltuples::bigint FROM pg_class WHERE oid = 'memories'::regclass").fetchone()[0]
        with conn.transaction():
            set_tenant(conn, a.id)
            visible = conn.execute("SELECT count(*) FROM memories").fetchone()[0]
            est = conn.execute("EXPLAIN (FORMAT JSON) SELECT * FROM memories").fetchone()[0][0]["Plan"]["Plan Rows"]
        obs.append(f"pg_class.reltuples for memories reads {reltuples} (both tenants, after ANALYZE) while tenant A "
                   f"sees {visible}; EXPLAIN estimates {est} rows. Catalog row counts span tenants")
        estimates = {}
        for word in ("secret", "zzzunusedword"):
            with conn.transaction():
                set_tenant(conn, a.id)
                plan = conn.execute("EXPLAIN (FORMAT JSON) SELECT id FROM memories WHERE to_tsvector('simple', "
                                    "claim) @@ plainto_tsquery('simple', %s)", [word]).fetchone()[0]
                actual = len(conn.execute("SELECT id FROM memories WHERE to_tsvector('simple', claim) @@ "
                                          "plainto_tsquery('simple', %s)", [word]).fetchall())
            estimates[word] = (plan[0]["Plan"]["Plan Rows"], actual)
        obs.append("planner estimate under tenant A (estimated, returned) for a word only tenant B's memories hold, "
                   f"'secret': {estimates['secret']}; for a word no tenant holds: {estimates['zzzunusedword']}"
                   + (". The estimates differ, so EXPLAIN tells tenant A whether some other tenant's memories use "
                      "a word" if estimates['secret'][0] != estimates['zzzunusedword'][0] else
                      ". The estimates are equal: EXPLAIN does not separate the two words here"))

        def oracle(statement, params) -> str:
            return under(a.id, statement, params)[1] or "accepted"

        fk_b = oracle("INSERT INTO workareas (tenant_id, parent_id, name, status) VALUES (%s, %s, 'probe', "
                      "'development')", [a.id, b.universe])
        fk_none = oracle("INSERT INTO workareas (tenant_id, parent_id, name, status) VALUES (%s, %s, 'probe', "
                         "'development')", [a.id, uuid.uuid4()])
        obs.append(f"foreign-key existence oracle: a tenant A workarea naming tenant B's universe as parent is "
                   f"{fk_b}; naming a random id gives {fk_none}. The FK check runs without RLS, so tenant A can "
                   "test whether an id exists in another tenant and can store a cross-tenant parent_id (the "
                   "adapter's ancestry walk refuses such a chain at read time)")
        ev_b = oracle("INSERT INTO memories (tenant_id, workarea_id, claim, claim_digest, status, created_event_id, "
                      "claim_key, author_principal, author_key_id, version, kind, body_bytes) VALUES (%s, %s, 'probe', "
                      "'\\x00', 'held', %s, 'probe', 'probe', %s, 1, 'memory', 5)", [a.id, a.child, b.events[4], a.writer])
        obs.append(f"a tenant A memory whose created_event_id names a tenant B journal event is {ev_b} "
                   "(memories.created_event_id references journal(event_id) alone)")
        rq_b = oracle("INSERT INTO review_queue (memory_id, tenant_id) VALUES (%s, %s)", [b.m1, a.id])
        obs.append(f"a tenant A review_queue row naming tenant B's memory is {rq_b} "
                   "(review_queue.memory_id references memories(id) alone)")
        uq = oracle("INSERT INTO api_keys (tenant_id, hash, role, principal, issued_seq) VALUES (%s, %s, 'reader', "
                    "'probe', 1)", [a.id, key_hash("B", "writer")])
        obs.append(f"unique-index existence oracle: a tenant A key row carrying tenant B's key hash gives {uq}; "
                   "the global UNIQUE on api_keys.hash answers whether a hash exists anywhere (it is the SHA-256 of "
                   "a secret key value, so the oracle needs the value)")
        with conn.transaction():
            set_tenant(conn, a.id)
            conn.execute("SELECT set_config('oml.startup_check', 'on', true)")
            startup = conn.execute("SELECT id, name FROM tenants").fetchall()
        obs.append(f"0003's lead_startup_enumeration: a session that sets oml.startup_check = 'on' reads "
                   f"{len(startup)} tenant rows (ids and names, {rows_tainted(startup, needles)} of tenant B) "
                   "whatever oml.tenant_id says; it is a second server-set GUC with the same trust as oml.tenant_id")
        with conn.transaction():
            set_tenant(conn, a.id)
            conn.execute("SELECT set_config('oml.api_key_hash', %s, true)", [key_hash("B", "writer").hex()])
            resolved = conn.execute("SELECT id, tenant_id FROM api_keys").fetchall()
        obs.append(f"0001's key_resolution policy: with oml.api_key_hash set to tenant B's key hash, a tenant A "
                   f"session reads {rows_tainted(resolved, needles)} tenant B key row(s); the policy opens the key "
                   "a request presents, by design")
        for o in obs:
            s.observations.append(f"[{role}] {o}")


# ---------------------------------------------------------------------------
# Orchestration
# ---------------------------------------------------------------------------


def server_facts(conn: psycopg.Connection) -> dict[str, str]:
    def q(statement: str) -> str:
        return conn.execute(statement).fetchone()[0]

    facts = {
        "version": q("SELECT version()"),
        "server_version": q("SHOW server_version"),
        "row_security": q("SHOW row_security"),
        "plan_cache_mode": q("SHOW plan_cache_mode"),
        "default_transaction_isolation": q("SHOW default_transaction_isolation"),
        "server_encoding": q("SHOW server_encoding"),
    }
    vector = conn.execute("SELECT default_version FROM pg_available_extensions WHERE name = 'vector'").fetchone()
    facts["pgvector available"] = vector[0] if vector else "no"
    return facts


def host_facts() -> str:
    cpu = "unknown CPU"
    with contextlib.suppress(OSError):
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    mem = ""
    with contextlib.suppress(OSError, ValueError, IndexError):
        kb = int(Path("/proc/meminfo").read_text().split()[1])
        mem = f", {kb / 1024 / 1024:.1f} GiB RAM"
    return f"{platform.system()} {platform.release()} {platform.machine()}, {os.cpu_count()} x {cpu}{mem}"


def print_finding(s: Scenario, versions: str, settings: str, environment: str) -> None:
    failing = [key for ok, _, key in s.checks if not ok]
    recorded = failing and all(key in RECORDED_FAILURES for key in failing)
    # A failure is "ADR amendment needed" until the maintainer decides otherwise; the harness
    # never reads a failure as needing none (docs/engineering/spikes/index.md).
    action = {"pass": "none",
              "fail": f"ADR amendment needed, against {s.adr_clause}; awaiting maintainer decision"
                      + (f"; recorded at {', '.join(RECORDED_FAILURES[k] for k in failing)}" if recorded
                         else "; no finding records this failure yet"),
              "inconclusive": "re-run: a control did not leak, so a check could not have failed",
              "not run": "run where the harness can mint roles (CI job rls-spike)"}[s.verdict]
    print(f"\nspike-finding: {s.id} — {s.title}")
    print(f"date:          {date.today().isoformat()}")
    print(f"versions:      {versions}")
    print(f"settings:      {'; '.join([settings, *s.settings])}")
    print(f"environment:   {environment}")
    print(f"verdict:       {s.verdict}" + (f" — {s.not_run_reason}" if not s.ran else "")
          + (f"; unprobed for row visibility, no seeded row: {', '.join(s.unprobed)}" if s.unprobed else ""))
    print(f"adr-clause:    {s.adr_clause}")
    print(f"action:        {action}")
    for ok, text, key in s.checks:
        mark = "ok" if ok else (f"FAIL, recorded: {RECORDED_FAILURES[key]}" if key in RECORDED_FAILURES else "FAIL")
        print(f"  check   [{mark}] {text}")
    for leaked, text in s.controls:
        print(f"  control [{'leaked as required' if leaked else 'DID NOT LEAK'}] {text}")
    for text in s.observations:
        print(f"  observed  {text}")


def apply_migrations(owner_ci: str, migrations: Path, a: Tenant, b: Tenant) -> tuple[list[tuple[str, bool, str]], list[tuple[bool, str]]]:
    log: list[tuple[str, bool, str]] = []
    backfill: list[tuple[bool, str]] = []
    files = sorted(migrations.glob("*.sql"))
    with connect(owner_ci) as conn:
        for path in files:
            if path.name.startswith(SEED_BEFORE):
                for t in (a, b):
                    seed_before_0004(conn, t)
            try:
                with conn.transaction():
                    conn.execute(path.read_text(encoding="utf-8"))
                log.append((path.stem, True, ""))
            except psycopg.Error as e:
                log.append((path.stem, False, f"{e.sqlstate} {e.diag.message_primary}"))
                return log, backfill
        for t in (a, b):
            with tenant_tx(conn, t.id):
                keys = dict((r[0], (r[1], r[2])) for r in conn.execute(
                    "SELECT id, issued_seq, issued_by FROM api_keys").fetchall())
                mems = conn.execute(
                    "SELECT id, version, kind, body_bytes = octet_length(normalize(claim, NFC)), is_tip, "
                    "promoted_at IS NOT NULL FROM memories ORDER BY id").fetchall()
                idem = conn.execute("SELECT response ->> 'version' FROM idempotency").fetchall()
            keys_ok = keys == {t.writer: (2, None), t.dreamer: (3, t.writer)}
            expect = {t.m1: (1, "memory", True, True, True), t.m2: (1, "memory", True, False, False),
                      t.m3: (1, "memory", True, True, True)}
            mems_ok = {r[0]: tuple(r[1:]) for r in mems} == expect
            backfill.append((keys_ok and mems_ok and idem == [("1",)],
                             f"0004 and 0005 backfilled tenant {t.letter}'s rows under FORCE: issued_seq/issued_by "
                             f"{'as journalled' if keys_ok else keys}, version/kind/body_bytes/is_tip/promoted_at "
                             f"{'as derived' if mems_ok else mems}, idempotency version {idem}"))
        for t in (a, b):
            seed_after_migrations(conn, t)
        conn.execute("ANALYZE")
    return log, backfill


def shown(path: Path) -> str:
    names = sorted(p.stem.split("_")[0] for p in path.glob("*.sql"))
    where = path.resolve().relative_to(ROOT) if path.resolve().is_relative_to(ROOT) else path
    return f"{where}/{names[0]}-{names[-1]}" if names else f"{where} (none)"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dsn", default=os.environ.get("OML_SPIKE_PG_DSN", DEFAULT_DSN),
                        help="a role that can CREATE DATABASE; a superuser also runs S3.2")
    parser.add_argument("--require-all", action="store_true", help="a scenario that did not run is a failure")
    parser.add_argument("--migrations", type=Path, default=MIGRATIONS,
                        help="the directory of *.sql migrations to apply (default: the repository's)")
    args = parser.parse_args()
    # S3.3 returns a connection inside an open transaction on purpose; the pool's warning
    # about rolling it back is the behaviour under test, and the check reports it.
    logging.getLogger("psycopg.pool").setLevel(logging.ERROR)

    a, b = make_tenant("A"), make_tenant("B")
    suffix = secrets.token_hex(4)
    dbname = f"oml_spike_rls_{suffix}"
    owner = service = control_role = None
    admin = connect(args.dsn)
    me, superuser = admin.execute(
        "SELECT rolname, rolsuper FROM pg_roles WHERE rolname = current_user").fetchone()
    facts = server_facts(admin)
    split = bool(superuser)
    target = conninfo_to_dict(args.dsn)
    where = f"{target.get('host', 'local socket')}:{target.get('port', '5432')}"
    scenarios: list[Scenario] = []
    try:
        if split:
            owner, service, control_role = (f"oml_spike_{suffix}_{r}" for r in ("owner", "service", "bypass"))
            pw = {r: secrets.token_hex(16) for r in (owner, service)}
            for r in (owner, service):
                admin.execute(sql.SQL("CREATE ROLE {} LOGIN PASSWORD {} NOSUPERUSER NOCREATEDB NOCREATEROLE "
                                      "NOBYPASSRLS").format(sql.Identifier(r), sql.Literal(pw[r])))
            admin.execute(sql.SQL("CREATE ROLE {} NOLOGIN BYPASSRLS").format(sql.Identifier(control_role)))
            admin.execute(sql.SQL("CREATE DATABASE {} OWNER {}").format(sql.Identifier(dbname), sql.Identifier(owner)))
            owner_ci = make_conninfo(args.dsn, dbname=dbname, user=owner, password=pw[owner])
            service_ci = make_conninfo(args.dsn, dbname=dbname, user=service, password=pw[service])
            owner_name = owner
        else:
            admin.execute(sql.SQL("CREATE DATABASE {}").format(sql.Identifier(dbname)))
            owner_ci = make_conninfo(args.dsn, dbname=dbname)
            service_ci = None
            owner_name = me
        admin_db_ci = make_conninfo(args.dsn, dbname=dbname)

        migration_log, backfill = apply_migrations(owner_ci, args.migrations, a, b)
        if not all(ok for _, ok, _ in migration_log):
            print(f"spike_rls: migrations failed: {migration_log}", file=sys.stderr)
            return 1
        if split:
            with connect(owner_ci) as conn:
                s_id = sql.Identifier(service)
                conn.execute(sql.SQL("GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO {}").format(s_id))
                conn.execute(sql.SQL("REVOKE UPDATE, DELETE ON journal FROM {}").format(s_id))
                conn.execute(sql.SQL("GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO {}").format(s_id))
                conn.execute(sql.SQL("GRANT SELECT ON memories TO {}").format(sql.Identifier(control_role)))

        roles = {f"owner {owner_name}": owner_ci}
        if split:
            roles[f"service {service}"] = service_ci
        scenarios.append(s3_1(owner_ci, a, b, owner_name))
        if split:
            scenarios.append(s3_2(admin_db_ci, owner_ci, service_ci, owner, service, a, b,
                                  migration_log, backfill, control_role))
        else:
            s = Scenario("S3.2", "Owner/migration-role split; application roles carry no BYPASSRLS",
                         "ADR-0007 d2 (no BYPASSRLS role), through the role split of security.md (commitments C6)",
                         ran=False, not_run_reason=f"the connecting role {me} is not a superuser, so the harness "
                         "cannot mint the owner and service roles; what did run: migrations "
                         + ", ".join(n for n, ok, _ in migration_log if ok) + " succeeded under the owner role "
                         "with FORCE binding it, and " + "; ".join(t for _, t in backfill))
            scenarios.append(s)
        scenarios.append(s3_3(roles, a, b))
        scenarios.append(s3_4(roles, a, b))
    finally:
        with contextlib.suppress(psycopg.Error):
            admin.execute(sql.SQL("DROP DATABASE IF EXISTS {} WITH (FORCE)").format(sql.Identifier(dbname)))
        for r in (owner, service, control_role):
            if r:
                with contextlib.suppress(psycopg.Error):
                    admin.execute(sql.SQL("DROP ROLE IF EXISTS {}").format(sql.Identifier(r)))
        left = admin.execute("SELECT count(*) FROM pg_database WHERE datname = %s", [dbname]).fetchone()[0]
        admin.close()

    versions = (f"PostgreSQL {facts['server_version']} ({facts['version'].split(',')[0]}); pgvector "
                f"{facts['pgvector available']}; psycopg {psycopg.__version__} (libpq "
                f"{psycopg.pq.version() // 10000}.{psycopg.pq.version() % 100}); psycopg-pool "
                f"{psycopg_pool.__version__}; Python {platform.python_version()}")
    settings = (f"server defaults row_security={facts['row_security']}, plan_cache_mode={facts['plan_cache_mode']}, "
                f"default_transaction_isolation={facts['default_transaction_isolation']}, "
                f"server_encoding={facts['server_encoding']}; migrations {shown(args.migrations)} "
                f"applied by the harness in one transaction each, two tenants seeded after 0003")
    environment = (f"{host_facts()}; Postgres at {where}; connecting role {me} "
                   f"({'superuser: split roles minted' if split else 'not a superuser: owner-only run'}); "
                   f"throwaway database {dbname} ({'dropped' if not left else 'NOT DROPPED'})")
    for s in scenarios:
        print_finding(s, versions, settings, environment)

    verdicts = {s.id: s.verdict for s in scenarios}
    print(f"\nspike_rls: verdicts {verdicts}")
    results = {key: ok for sc in scenarios for ok, _, key in sc.checks if key}
    unexpected = [text for sc in scenarios for ok, text, key in sc.checks
                  if not ok and key not in RECORDED_FAILURES]
    stale = [key for key in RECORDED_FAILURES if results.get(key) is not False]
    inconclusive = [i for i, v in verdicts.items() if v == "inconclusive"]
    skipped = [i for i, v in verdicts.items() if v == "not run"]
    recorded = [key for key in RECORDED_FAILURES if results.get(key) is False]
    if recorded:
        print(f"spike_rls: recorded failures reproduced: {recorded}")
    problems = []
    if unexpected:
        problems.append(f"{len(unexpected)} failed check(s) no finding records: {unexpected}")
    if stale:
        problems.append(f"recorded failure(s) that did not reproduce, so their finding needs re-recording: {stale}")
    if inconclusive:
        problems.append(f"inconclusive: {inconclusive}")
    if args.require_all and skipped:
        problems.append(f"not run under --require-all: {skipped}")
    if left:
        problems.append(f"throwaway database {dbname} was not dropped")
    if problems:
        sys.stdout.flush()
        print("spike_rls: FAIL: " + "; ".join(problems), file=sys.stderr)
        return 1
    unprobed = sorted({t for sc in scenarios for t in sc.unprobed})
    print("spike_rls: OK: every check passed or reproduced its recorded finding"
          + (f"; not run: {skipped}" if skipped else "")
          + (f"; unprobed for row visibility (no seeded row): {unprobed}" if unprobed else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
