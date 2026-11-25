"""The read-only production counts (ADR-0097 d27 item 1) only read: the SQL
runs in one READ ONLY transaction that ends in ROLLBACK, holds no statement
that writes or changes the schema, and the wrapper opens a session that is
read-only by default."""

import re
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


if __name__ == "__main__":
    unittest.main()
