import tempfile
import unittest
from pathlib import Path

import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import lint_practices as L  # noqa: E402

PART = "CREATE TABLE memories (id int, tenant_id int) PARTITION BY LIST (tenant_id);\n"


def tree(files: dict[str, str]) -> Path:
    d = Path(tempfile.mkdtemp())
    for name, text in files.items():
        p = d / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)
    return d


class Migrations(unittest.TestCase):
    def run_lint(self, sql, **extra):
        return L.lint_migrations(tree({"migrations/0001_p.sql": PART, "migrations/0002_x.sql": sql, **extra}), False)

    def test_a_migration_without_lock_timeout_is_flagged(self):
        self.assertTrue(any("lock_timeout" in p for p in self.run_lint("ALTER TABLE t ADD COLUMN a int;")))

    def test_a_constraint_added_valid_is_flagged(self):
        sql = "SET LOCAL lock_timeout='5s';\nALTER TABLE t ADD CONSTRAINT c CHECK (a > 0);"
        self.assertTrue(any("NOT VALID" in p for p in self.run_lint(sql)))

    def test_not_valid_without_validate_is_flagged_and_a_later_validate_clears_it(self):
        sql = "SET LOCAL lock_timeout='5s';\nALTER TABLE t ADD CONSTRAINT c CHECK (a > 0) NOT VALID;"
        self.assertTrue(any("never VALIDATEd" in p for p in self.run_lint(sql)))
        later = {"migrations/0003_v.sql": "SET LOCAL lock_timeout='5s';\nALTER TABLE t VALIDATE CONSTRAINT c;"}
        self.assertEqual([], [p for p in self.run_lint(sql, **later) if "0002" in p])

    def test_an_index_on_a_partitioned_table_needs_only_and_attach(self):
        head = "SET LOCAL lock_timeout='5s';\n"
        self.assertTrue(any("ON ONLY" in p for p in self.run_lint(head + "CREATE INDEX i ON memories (id);")))
        self.assertTrue(any("ATTACH" in p for p in self.run_lint(head + "CREATE INDEX i ON ONLY memories (id);")))
        ok = head + "CREATE INDEX i ON ONLY memories (id);\nALTER INDEX i ATTACH PARTITION p_i;"
        self.assertEqual([], [p for p in self.run_lint(ok) if "0002" in p])

    def test_the_allow_list_and_a_reasoned_escape_clear_a_migration(self):
        root = tree({"migrations/0001_p.sql": "ALTER TABLE t ADD COLUMN a int;",
                     "scripts/lint_allow/migrations.txt": "0001_p.sql\n"})
        self.assertEqual([], L.lint_migrations(root, True))
        self.assertTrue(L.lint_migrations(root, False))
        esc = tree({"migrations/0001_p.sql": "-- lint:allow lock_timeout new table only\nCREATE TABLE a (i int);"})
        self.assertEqual([], L.lint_migrations(esc, False))


class Ports(unittest.TestCase):
    def test_a_default_to_5432_in_a_test_is_flagged_and_docs_and_annotations_are_not(self):
        root = tree({
            "crates/x/tests/a.rs": 'let u = "postgres://u@127.0.0.1:5432/db";',
            "crates/x/tests/b.rs": 'let u = "postgres://u@127.0.0.1:5432/db"; // lint:allow-5432 ci service',
            "scripts/c.sh": "PORT=${PGPORT:-5432}",
            "docs/d.md": "localhost:5432",
            "crates/x/src/e.rs": 'let u = "localhost:5432";',
        })
        hits = L.lint_ports(root, False)
        self.assertEqual(sorted(h.split(":")[0] for h in hits), ["crates/x/tests/a.rs", "scripts/c.sh"])

    def test_the_allow_list_clears_a_path(self):
        root = tree({"scripts/c.sh": "localhost:5432", "scripts/lint_allow/ports.txt": "scripts/c.sh\n"})
        self.assertEqual([], L.lint_ports(root, True))


class Like(unittest.TestCase):
    def test_like_in_candidate_sql_is_flagged_but_table_like_comments_and_annotations_are_not(self):
        src = "\n".join([
            'let a = "SELECT 1 WHERE name LIKE $1";',
            'let b = "SELECT 1 WHERE name ilike $1";',
            "// a LIKE in a comment",
            'let c = "CREATE TABLE p (LIKE memories INCLUDING ALL)";',
            "// lint:allow-like literal prefix, reviewed against RLS",
            'let d = "SELECT 1 WHERE name LIKE $1";',
        ])
        root = tree({"crates/adapters/oml-pg/src/lib.rs": src})
        self.assertEqual([1, 2], [int(h.split(":")[1]) for h in L.lint_like(root, False)])


if __name__ == "__main__":
    unittest.main()
