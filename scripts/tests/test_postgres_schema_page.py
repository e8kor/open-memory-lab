"""The PostgreSQL schema page (docs/data/postgres-schema.md): scripts/gen_postgres_schema.py.

The rendering tests need no database: they render from a catalog snapshot. The last class applies
the migrations to a real PostgreSQL, and runs only where OML_SCHEMA_DATABASE_URL names a superuser
URL; CI's "schema page matches the migrations" job runs the same check through the script itself.
"""

import contextlib
import copy
import io
import os
import pathlib
import random
import re
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import gen_postgres_schema as G  # noqa: E402


def table(name, kind="r", comment=None, partition_of=None, bound=None, grants=(), policies=(), checks=(),
          column_grants=()):
    return {
        "name": name, "kind": kind, "owner": "owner", "comment": comment,
        "rls": kind in ("r", "p"), "force_rls": kind in ("r", "p"),
        "partition_of": partition_of, "partition_bound": bound,
        "partition_key": "LIST (tenant_id)" if kind == "p" else None,
        "columns": [] if kind == "S" else [
            {"name": "tenant_id", "type": "uuid", "not_null": True, "default": None,
             "generated": False, "identity": False, "comment": "The tenant | its id."},
            {"name": "note", "type": "text", "not_null": False, "default": "'x'::text",
             "generated": False, "identity": False, "comment": None},
        ],
        "constraints": [{"name": n, "type": "c", "def": d} for n, d in checks]
        + ([{"name": f"{name}_pkey", "type": "p", "def": "PRIMARY KEY (tenant_id)"}] if kind in ("r", "p") else []),
        "policies": list(policies),
        "grants": [{"grantee": g, "privilege": p, "grantable": False} for g, p in grants],
        "column_grants": [{"column": c, "grantee": g, "privilege": p, "grantable": False}
                          for c, g, p in column_grants],
    }


POLICY = {"name": "tenant_isolation", "command": "*", "permissive": True, "roles": [],
          "using": "tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid",
          "check": "tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid"}


def snapshot():
    return {
        "last_migration": "0002_second.sql",
        "extensions": ["ltree"],
        "relations": [
            table("tenants", comment="Tenants. Owning page: docs/data/postgres-schema.md.",
                  grants=[("serving", "UPDATE"), ("serving", "SELECT"), ("lake writer", "SELECT")],
                  policies=[POLICY], checks=[("tenants_note", "CHECK (note ~ '^[a-z]|[0-9]$')")]),
            table("memories", kind="p", comment="Claims (ADR-0034 d11).",
                  grants=[("serving", "INSERT"), ("serving", "SELECT")], policies=[POLICY]),
            table("memories_t_00", partition_of="memories", bound="FOR VALUES IN ('00')"),
            table("outbox_seq_seq", kind="S", grants=[("serving", "USAGE"), ("lake writer", "SELECT")]),
            table("_sqlx_migrations"),
            table("artifact_keys", comment="Keys.", policies=[POLICY],
                  column_grants=[("tenant_id", "serving", "SELECT"), ("note", "serving", "UPDATE"),
                                 ("note", "serving", "SELECT"), ("tenant_id", "lake writer", "SELECT")],
                  grants=[("lake writer", "UPDATE")]),
        ],
    }


def shuffled(snap, seed):
    rng = random.Random(seed)
    out = copy.deepcopy(snap)
    rng.shuffle(out["relations"])
    for rel in out["relations"]:
        rng.shuffle(rel["grants"])
    return out


class Rendering(unittest.TestCase):
    def test_the_page_is_the_same_whatever_order_the_catalog_answers_in(self):
        first = G.render(snapshot())
        for seed in range(5):
            self.assertEqual(first, G.render(shuffled(snapshot(), seed)))

    def test_every_table_has_a_heading_and_a_privileges_row_and_a_partition_has_neither(self):
        page = G.render(snapshot())
        for name in ("tenants", "memories", "_sqlx_migrations"):
            self.assertIn(f"\n### {name}\n", page)
            self.assertIn(f"| [`{name}`](#{name}) | owns |", page)
        self.assertNotIn("### memories_t_00", page)
        self.assertIn("- `memories_t_00` `FOR VALUES IN ('00')`", page)
        self.assertNotIn("### outbox_seq_seq", page)
        self.assertIn("| `outbox_seq_seq` | owns | USAGE | SELECT |", page)

    def test_grants_are_listed_per_role_in_privilege_order(self):
        page = G.render(snapshot())
        self.assertIn("| [`tenants`](#tenants) | owns | SELECT, UPDATE | SELECT |", page)
        self.assertIn("Grants: serving: SELECT, UPDATE; lake writer: SELECT.", page)
        self.assertIn("No table or sequence grants anything to `PUBLIC`.", page)

    def test_a_column_grant_is_listed_with_its_columns_and_a_table_wide_grant_without(self):
        page = G.render(snapshot())
        self.assertIn("Grants: serving: SELECT (note, tenant_id), UPDATE (note); "
                      "lake writer: SELECT (tenant_id), UPDATE.", page)
        self.assertIn("| [`artifact_keys`](#artifact_keys) | owns | SELECT (note, tenant_id), UPDATE (note) "
                      "| SELECT (tenant_id), UPDATE |", page)

    def test_a_column_grant_to_a_role_the_run_did_not_mint_stops_the_run(self):
        raw = {"relations": [table("tenants", column_grants=[("note", "someone_else", "SELECT")])]}
        with self.assertRaises(RuntimeError):
            G.relabel(raw, {"owner": "owner"})

    def test_checks_policies_and_the_row_level_security_mode_are_written_out(self):
        page = G.render(snapshot())
        self.assertIn("- `tenants_note`: `CHECK (note ~ '^[a-z]\\|[0-9]$')`", page)
        self.assertIn("Row-level security: enabled and forced, so the owner is bound too.", page)
        self.assertIn("- Policy `tenant_isolation`, ALL for PUBLIC, permissive:", page)
        self.assertIn("Kind: partitioned table, `PARTITION BY LIST (tenant_id)`.", page)

    def test_a_comment_loses_its_self_link_and_its_handles_and_a_pipe_is_escaped(self):
        page = G.render(snapshot())
        self.assertIn("### tenants\n\nTenants.\n", page)
        self.assertIn("### memories\n\nClaims.\n", page)
        self.assertIn("| The tenant \\| its id. |", page)

    def test_a_role_the_run_did_not_mint_stops_the_run(self):
        raw = {"relations": [table("tenants", grants=[("someone_else", "SELECT")])]}
        raw["relations"][0]["owner"] = "m_owner"
        with self.assertRaisesRegex(RuntimeError, "someone_else"):
            G.relabel(raw, {"m_owner": "owner"})

    def test_the_lake_writer_grants_are_read_from_its_source(self):
        self.assertEqual([
            "GRANT SELECT ON tenants, journal, outbox, memories, claim_edges, brief_edges, "
            "recall_events, recall_outcomes TO {role}",
            "GRANT SELECT, INSERT, UPDATE ON artifact_keys, lake_dead_letter TO {role}",
            "GRANT SELECT ON SEQUENCE outbox_seq_seq TO {role}",
        ], G.lake_grants())

    def test_a_lake_writer_entry_that_is_not_a_format_call_stops_the_run(self):
        src = G.LAKE_SOURCE.read_text().replace(
            '            format!("GRANT SELECT ON SEQUENCE {} TO {role}", "outbox_seq_seq"),',
            '            format!("GRANT SELECT ON SEQUENCE {} TO {role}", "outbox_seq_seq"),\n'
            '            String::from("GRANT SELECT ON usage_daily TO r"),')
        self.assertNotEqual(src, G.LAKE_SOURCE.read_text())
        with tempfile.TemporaryDirectory() as d:
            path = pathlib.Path(d) / "source.rs"
            path.write_text(src)
            with mock.patch.object(G, "LAKE_SOURCE", path), mock.patch.object(G.docgen, "ROOT", pathlib.Path(d)):
                with self.assertRaisesRegex(SystemExit, "4 entries and 3 were read"):
                    G.lake_grants()

    def test_top_level_entries_ignore_commas_inside_strings_and_brackets(self):
        self.assertEqual(['f("a, b", (1, 2))', "g([3, 4])", '"x,y"'],
                         G.top_level_entries(' f("a, b", (1, 2)),\n g([3, 4]), "x,y", '))


class Connection(unittest.TestCase):
    def test_an_identifier_is_quoted_and_its_quotes_doubled(self):
        self.assertEqual('"oml_x"', G.ident("oml_x"))
        self.assertEqual('"a""b"', G.ident('a"b'))
        self.assertEqual("'it''s'", G.literal("it's"))

    def test_the_password_goes_to_pgpassword_and_never_into_the_url(self):
        url, env = G.with_user("postgres://admin:s%40cret@127.0.0.1:5432/oml_dev", None, None)
        self.assertEqual("postgres://admin@127.0.0.1:5432/oml_dev", url)
        self.assertEqual("s@cret", env["PGPASSWORD"])
        url, env = G.with_user("postgres://admin:s@127.0.0.1:5432/oml_dev", "minted", "db1", "run-pw")
        self.assertEqual("postgres://minted@127.0.0.1:5432/db1", url)
        self.assertEqual("run-pw", env["PGPASSWORD"])

    def test_a_password_in_the_query_string_is_refused(self):
        with self.assertRaisesRegex(RuntimeError, "query string"):
            G.with_user("postgres://admin@127.0.0.1/oml_dev?password=s", None, None)

    def test_database_url_alone_names_no_server(self):
        with mock.patch.dict(os.environ, {"DATABASE_URL": "postgres://oml@127.0.0.1/oml_dev"}, clear=False):
            os.environ.pop("OML_SCHEMA_DATABASE_URL", None)
            with mock.patch.object(G, "snapshot") as snap, contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(2, G.main(["--check"]))
            snap.assert_not_called()

    def test_each_cleanup_step_runs_and_its_failure_is_named_after_the_original_error(self):
        ran = []

        def fake_psql(url, env, *args, sql=None):
            ran.append(sql)
            if sql.startswith("CREATE DATABASE"):
                raise RuntimeError("no space left")
            if sql.startswith("DROP ROLE") and "serving" in sql:
                raise RuntimeError("role is in use")
            return ""

        with mock.patch.object(G, "psql", fake_psql):
            with self.assertRaisesRegex(RuntimeError, "no space left") as caught:
                G.snapshot("postgres://admin@127.0.0.1/oml_dev")
        drops = [s for s in ran if s.startswith("DROP")]
        self.assertEqual(3, len(drops))
        self.assertFalse(any(s.startswith("DROP DATABASE") for s in drops))
        self.assertEqual(1, len(caught.exception.__notes__))
        self.assertIn("role is in use", caught.exception.__notes__[0])
        self.assertNotIn("_owner", caught.exception.__notes__[0])


def tables_the_migrations_leave() -> set[str]:
    """Each table a CREATE TABLE names, followed through RENAME TO and DROP TABLE, file by file."""
    tables: set[str] = set()
    pattern = re.compile(
        r"\bCREATE\s+TABLE(?:\s+IF\s+NOT\s+EXISTS)?\s+(\w+)"
        r"|\bALTER\s+TABLE(?:\s+IF\s+EXISTS)?(?:\s+ONLY)?\s+(\w+)\s+RENAME\s+TO\s+(\w+)"
        r"|\bDROP\s+TABLE(?:\s+IF\s+EXISTS)?\s+([\w, ]+)", re.I)
    for path in G.migration_files():
        text = re.sub(r"--[^\n]*", "", path.read_text())
        for m in pattern.finditer(text):
            if m.group(1):
                tables.add(m.group(1).lower())
            elif m.group(2):
                tables.discard(m.group(2).lower())
                tables.add(m.group(3).lower())
            else:
                tables.difference_update(n.strip().lower() for n in m.group(4).split(","))
    return tables


class CommittedPage(unittest.TestCase):
    def test_the_committed_page_has_a_heading_for_every_table_the_migrations_leave_and_no_other(self):
        headings = {line[4:].strip() for line in G.OUT.read_text().splitlines() if line.startswith("### ")}
        self.assertEqual(tables_the_migrations_leave() | {"_sqlx_migrations"}, headings)

    def test_the_committed_page_names_the_last_migration(self):
        self.assertIn(f"after `{G.migration_files()[-1].name}`", G.OUT.read_text())


@unittest.skipUnless(os.environ.get("OML_SCHEMA_DATABASE_URL"), "needs OML_SCHEMA_DATABASE_URL, a PostgreSQL superuser URL")
class AgainstPostgres(unittest.TestCase):
    def test_two_runs_render_the_same_page_and_it_matches_the_committed_one(self):
        url = os.environ["OML_SCHEMA_DATABASE_URL"]
        snap = G.snapshot(url)
        first, second = G.render(snap), G.render(G.snapshot(url))
        self.assertEqual(first, second)
        catalog = {r["name"] for r in snap["relations"] if r["kind"] in ("r", "p") and not r["partition_of"]}
        headings = {line[4:].strip() for line in first.splitlines() if line.startswith("### ")}
        self.assertEqual(catalog, headings)
        self.assertEqual(G.OUT.read_text(), first.rstrip("\n") + "\n")


if __name__ == "__main__":
    unittest.main()
