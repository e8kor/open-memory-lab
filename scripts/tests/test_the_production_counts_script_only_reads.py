"""The read-only production counts (ADR-0097 d27 item 1) only read: the SQL
runs in one READ ONLY transaction that ends in ROLLBACK, holds no statement
that writes or changes the schema, and the wrapper opens a session that is
read-only by default. They count as the NOSUPERUSER NOBYPASSRLS owner role
under FORCE row-level security, one tenant at a time; the wrapper refuses a
URL that carries a password and fails when no tenant is visible.

With OML_COUNTS_TEST_DATABASE_URL naming a seeded throwaway database as its
owner role, and its password in PGPASSFILE, the script also runs against it
and every count comes back for every tenant."""

import os
import re
import shutil
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

OPS = Path(__file__).resolve().parents[1] / "ops"
SQL = (OPS / "read_only_counts.sql").read_text()
SH = (OPS / "read_only_counts.sh").read_text()


def statements(sql: str) -> str:
    """The SQL without its comments."""
    return "\n".join(line.split("--", 1)[0] for line in sql.splitlines())


class TheProductionCountsScriptOnlyReads(unittest.TestCase):
    def test_the_sql_runs_in_one_read_only_transaction_ending_in_rollback(self):
        body = statements(SQL).strip()
        self.assertRegex(body, r"^BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY;")
        self.assertTrue(body.endswith("ROLLBACK;"), body[-40:])
        self.assertNotRegex(body, r"(?i)\bCOMMIT\b")

    def test_the_sql_holds_no_statement_that_writes(self):
        body = statements(SQL)
        for verb in ("INSERT", "UPDATE", "DELETE", "MERGE", "CREATE", "ALTER", "DROP",
                     "TRUNCATE", "GRANT", "REVOKE", "COPY", "SET ROLE"):
            self.assertIsNone(re.search(rf"(?i)\b{verb}\b", body), verb)

    def test_the_wrapper_opens_a_read_only_session_and_stops_on_an_error(self):
        self.assertIn("default_transaction_read_only=on", SH)
        self.assertIn("ON_ERROR_STOP=1", SH)
        self.assertIn("OML_COUNTS_DATABASE_URL", SH)

    def test_it_reports_every_count_adr_0097_lists(self):
        for count in ("unlinked_admin_keys", "service_admin_keys", "cross_root_grants",
                      "admin_minted_user_keys", "admin_minted_connect_codes",
                      "open_investigations", "admin_persons_ever"):
            self.assertIn(f"'{count}'", SQL)

    def test_it_lists_the_tenants_under_startup_check_and_counts_each_under_its_tenant_id(self):
        body = statements(SQL)
        self.assertRegex(body, r"SET LOCAL oml\.startup_check = 'on';")
        self.assertIn("format('SET LOCAL oml.startup_check = %L', '')", body)
        self.assertIn("format('SET LOCAL oml.tenant_id = %L', id)", body)
        self.assertRegex(body, r"FROM tenants\s+ORDER BY id\s+\\gexec")
        self.assertNotRegex(body, r"(?i)\bDO\b|\bFUNCTION\b|plpgsql")

    def test_the_wrapper_takes_no_password_on_its_command_line(self):
        self.assertNotIn("PGPASSWORD", SH)
        self.assertIn("PGPASSFILE", SH)
        self.assertNotIn("ROLLBACK,.", SH)


def run_wrapper(url: str, psql_output: str):
    """The wrapper run with a stand-in psql on PATH that prints
    psql_output and records its arguments; (exit code, stdout, stderr,
    the arguments psql was given)."""
    with tempfile.TemporaryDirectory() as tmp:
        bin_dir = Path(tmp)
        argv_file = bin_dir / "argv"
        psql = bin_dir / "psql"
        psql.write_text(
            "#!/bin/sh\n"
            f"printf '%s\\n' \"$@\" > '{argv_file}'\n"
            f"cat <<'OUT'\n{psql_output}OUT\n"
        )
        psql.chmod(psql.stat().st_mode | stat.S_IXUSR)
        env = {"PATH": f"{bin_dir}:/usr/bin:/bin", "OML_COUNTS_DATABASE_URL": url}
        done = subprocess.run(
            ["bash", str(OPS / "read_only_counts.sh")],
            env=env, capture_output=True, text=True, check=False,
        )
        argv = argv_file.read_text() if argv_file.exists() else ""
        return done.returncode, done.stdout, done.stderr, argv


class TheWrapperGuardsItsInputAndItsAnswer(unittest.TestCase):
    def test_a_url_with_a_password_is_refused_before_psql_runs(self):
        code, out, err, argv = run_wrapper("postgres://owner:secret@db/oml", "")
        self.assertEqual(code, 2, err)
        self.assertIn("PGPASSFILE", err)
        self.assertEqual((out, argv), ("", ""))

    def test_no_tenant_visible_exits_non_zero_and_prints_nothing(self):
        code, out, err, _ = run_wrapper("postgres://owner@db/oml", "count,tenant,value\n")
        self.assertEqual(code, 1, err)
        self.assertEqual(out, "")
        self.assertIn("no tenant is visible", err)

    def test_counts_pass_through_and_the_url_carries_no_password(self):
        rows = "count,tenant,value\nopen_investigations,acme,0\n"
        code, out, err, argv = run_wrapper("postgres://owner@db/oml", rows)
        self.assertEqual(code, 0, err)
        self.assertEqual(out, rows)
        self.assertIn("postgres://owner@db/oml", argv)


COUNTS = ("unlinked_admin_keys", "service_admin_keys", "cross_root_grants",
          "admin_minted_user_keys", "admin_minted_connect_codes",
          "open_investigations", "admin_persons_ever")


@unittest.skipUnless(os.environ.get("OML_COUNTS_TEST_DATABASE_URL") and shutil.which("psql"),
                     "set OML_COUNTS_TEST_DATABASE_URL to a seeded throwaway database's owner URL")
class TheCountsRunAsTheOwnerRoleUnderForceRowLevelSecurity(unittest.TestCase):
    def test_every_count_comes_back_for_every_tenant(self):
        url = os.environ["OML_COUNTS_TEST_DATABASE_URL"]
        role = subprocess.run(
            ["psql", "--no-psqlrc", "-tA", "--dbname", url, "-c",
             "SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname = current_user"],
            capture_output=True, text=True, check=True,
        ).stdout.strip()
        self.assertEqual(role, "f", "the owner role is NOSUPERUSER NOBYPASSRLS")
        done = subprocess.run(
            ["bash", str(OPS / "read_only_counts.sh")],
            env={**os.environ, "OML_COUNTS_DATABASE_URL": url},
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(done.returncode, 0, done.stderr)
        lines = done.stdout.splitlines()
        self.assertEqual(lines[0], "count,tenant,value")
        rows = [line.split(",") for line in lines[1:]]
        tenants = {tenant for _, tenant, _ in rows}
        self.assertTrue(tenants, done.stdout)
        for tenant in tenants:
            for count in COUNTS:
                self.assertEqual(
                    sum(1 for c, t, _ in rows if (c, t) == (count, tenant)), 1,
                    f"{count} for {tenant}: {done.stdout}",
                )
        self.assertTrue(all(v.isdigit() for _, _, v in rows), done.stdout)


if __name__ == "__main__":
    unittest.main()
