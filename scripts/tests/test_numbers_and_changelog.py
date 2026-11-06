import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import sys
S = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(S))
import assemble_changelog as A  # noqa: E402
import check_changelog_fragment as C  # noqa: E402
import check_numbers as CN  # noqa: E402
import oml_numbers as N  # noqa: E402
import reserve_number as R  # noqa: E402

ENV = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@x", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@x"}


def sh(cwd, *a):
    subprocess.run(a, cwd=cwd, check=True, capture_output=True, env=ENV)


def put(root, name, text):
    p = Path(root) / name
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)


def repo():
    """A clone whose origin has main (ADR-0001, migration 0001) and a branch other/x
    that adds ADR-0002 and migration 0002."""
    base = Path(tempfile.mkdtemp())
    o = base / "o"
    o.mkdir()
    sh(o, "git", "init", "-q", "-b", "main")
    put(o, "docs/adr/ADR-0001-first.md", "x")
    put(o, "migrations/0001_init.sql", "select 1;")
    sh(o, "git", "add", "-A"); sh(o, "git", "commit", "-qm", "m")
    sh(o, "git", "checkout", "-qb", "other/x")
    put(o, "docs/adr/ADR-0002-theirs.md", "x")
    put(o, "migrations/0002_theirs.sql", "select 1;")
    sh(o, "git", "add", "-A"); sh(o, "git", "commit", "-qm", "x")
    sh(o, "git", "checkout", "-q", "main")
    c = base / "c"
    sh(base, "git", "clone", "-q", str(o), "c")
    sh(c, "git", "checkout", "-qb", "mine")
    return c


class Numbers(unittest.TestCase):
    def test_only_adr_and_migration_numbers_are_tracked(self):
        c = repo()
        put(c, "docs/product/roadmap.md", "| FR-9 | nine | e | open |\n")
        sh(c, "git", "add", "-A"); sh(c, "git", "commit", "-qm", "fr")
        self.assertEqual(("adr", "migration"), N.KINDS)
        self.assertEqual({}, N.numbers_added(c, "origin/main", "HEAD"))
        self.assertEqual([], CN.check(c, "origin/main", "mine"))


    def test_reserve_takes_the_number_after_every_branch_and_a_second_reserve_takes_the_next(self):
        c = repo()
        R.main(["adr", "--branch", "mine", "--title", "a", "--root", str(c)])
        R.main(["adr", "--branch", "mine2", "--title", "b", "--root", str(c)])
        text = (c / "docs/NUMBERS.md").read_text()
        self.assertIn("| adr | 0003 | `mine` |", text)
        self.assertIn("| adr | 0004 | `mine2` |", text)

    def test_a_number_used_on_another_branch_is_flagged_and_a_reserved_one_passes(self):
        c = repo()
        put(c, "docs/adr/ADR-0002-mine.md", "x")
        sh(c, "git", "add", "-A"); sh(c, "git", "commit", "-qm", "a")
        self.assertTrue(any("ADR-0002" in p and "not reserved" in p for p in CN.check(c, "origin/main", "mine")))
        R.main(["adr", "--branch", "mine", "--title", "a", "--root", str(c)])
        sh(c, "git", "mv", "docs/adr/ADR-0002-mine.md", "docs/adr/ADR-0003-mine.md")
        sh(c, "git", "add", "-A"); sh(c, "git", "commit", "-qm", "b")
        self.assertEqual([], CN.check(c, "origin/main", "mine"))

    def test_a_number_the_registry_assigns_to_another_branch_is_flagged(self):
        c = repo()
        R.main(["migration", "--branch", "elsewhere", "--title", "t", "--root", str(c)])
        put(c, "migrations/0003_mine.sql", "select 1;")
        sh(c, "git", "add", "-A"); sh(c, "git", "commit", "-qm", "a")
        self.assertTrue(any("assigned to elsewhere" in p for p in CN.check(c, "origin/main", "mine")))

    def _clash(self):
        c = repo()
        put(c, "docs/adr/ADR-0002-mine.md", "x")
        R.main(["adr", "--branch", "mine", "--title", "a", "--root", str(c)])
        text = (c / "docs/NUMBERS.md").read_text().replace("| adr | 0003 |", "| adr | 0002 |")
        (c / "docs/NUMBERS.md").write_text(text)
        sh(c, "git", "add", "-A"); sh(c, "git", "commit", "-qm", "a")
        return c

    def test_a_branch_with_an_open_pull_request_holds_its_numbers(self):
        c = self._clash()
        self.assertTrue(any("also introduced on other/x" in p for p in CN.check(c, "origin/main", "mine", prs={"other/x": 1, "mine": 2})))

    def test_a_branch_whose_pull_request_is_closed_holds_no_numbers(self):
        c = self._clash()
        self.assertFalse(any("also introduced" in p for p in CN.check(c, "origin/main", "mine", prs={"mine": 2})))

    def test_a_branch_merged_into_main_holds_no_numbers(self):
        c = self._clash()
        sh(c, "git", "fetch", "-q", "origin")
        sh(c, "git", "update-ref", "refs/remotes/origin/main", "refs/remotes/origin/other/x")
        self.assertFalse(any("also introduced on other/x" in p for p in CN.check(c, "origin/main", "mine", prs={"other/x": 1, "mine": 2})))


class Changelog(unittest.TestCase):
    def test_assemble_inserts_above_the_newest_section_and_deletes_the_fragments(self):
        d = Path(tempfile.mkdtemp())
        put(d, "docs/changelog.md", "# Spec changelog\n\nintro\n\n## 0.5.8-design\n\nold\n")
        put(d, "changes/b.md", "second\n")
        put(d, "changes/a.md", "first\n")
        put(d, "changes/README.md", "readme\n")
        A.assemble(d / "docs/changelog.md", d / "changes", "0.5.9-design")
        text = (d / "docs/changelog.md").read_text()
        self.assertLess(text.index("## 0.5.9-design"), text.index("## 0.5.8-design"))
        self.assertLess(text.index("first"), text.index("second"))
        self.assertEqual(["README.md"], sorted(p.name for p in (d / "changes").iterdir()))

    def test_a_crates_change_without_a_fragment_fails_and_a_label_or_fragment_passes(self):
        code = [("M", "crates/a/src/lib.rs")]
        self.assertIsNotNone(C.verdict(code, []))
        self.assertIsNone(C.verdict(code, ["no-changelog"]))
        self.assertIsNone(C.verdict(code + [("A", "changes/78.md")], []))
        self.assertIsNotNone(C.verdict(code + [("A", "changes/README.md")], []))
        self.assertIsNone(C.verdict([("M", "README.md")], []))

    def test_a_release_branch_that_assembles_the_changelog_passes_and_any_other_release_branch_does_not(self):
        code = [("M", "crates/a/src/lib.rs")]
        assembled = code + [("M", "docs/changelog.md"), ("D", "changes/78.md")]
        self.assertIsNone(C.verdict(assembled, [], "release/v0.6.0"))
        self.assertIsNotNone(C.verdict(code, [], "release/v0.6.0"))
        self.assertIsNotNone(C.verdict(assembled, [], "feat/x"))


class DiskGuard(unittest.TestCase):
    def test_a_limit_below_usage_refuses(self):
        g = str(S / "disk_guard.sh")
        self.assertEqual(0, subprocess.run(["bash", g, ".", "100"], capture_output=True).returncode)
        self.assertEqual(1, subprocess.run(["bash", g, ".", "0"], capture_output=True).returncode)


if __name__ == "__main__":
    unittest.main()
