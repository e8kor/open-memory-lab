import contextlib
import io
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

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


def push_reservation(c, name, kind, number, title="theirs"):
    """A branch of origin that holds a reservation in its NUMBERS.md and nothing else, as a branch
    does between reserve_number.py and the pull request."""
    sh(c, "git", "checkout", "-qb", name, "origin/main")
    R.write_registry(c / "docs/NUMBERS.md", [
        {"kind": kind, "number": number, "branch": name, "pr": "", "title": title, "state": "reserved"}])
    sh(c, "git", "add", "-A"); sh(c, "git", "commit", "-qm", "reserve")
    sh(c, "git", "push", "-q", "origin", name)
    sh(c, "git", "checkout", "-q", "mine")
    sh(c, "git", "reset", "-q", "--hard", "HEAD")
    sh(c, "git", "fetch", "-q", "origin")


OPEN = {"other/x": 1, "mine": 2}


class EveryRemoteBranch(unittest.TestCase):
    def reserve(self, c, *extra):
        with mock.patch.object(N, "open_prs", return_value=OPEN):
            return R.main(["adr", "--branch", "mine", "--title", "a", "--root", str(c), *extra])

    def test_a_reservation_on_a_branch_with_no_pull_request_holds_its_number(self):
        c = repo()
        push_reservation(c, "res/y", "adr", 3)
        self.assertEqual(0, self.reserve(c))
        text = (c / "docs/NUMBERS.md").read_text()
        self.assertIn("| adr | 0004 | `mine` |", text)
        self.assertIn("| adr | 0003 | `res/y` |", text)

    def test_a_number_another_branch_without_a_pull_request_reserved_is_refused_and_the_branch_is_named(self):
        c = repo()
        put(c, "docs/adr/ADR-0003-mine.md", "x")
        R.main(["adr", "--branch", "mine", "--title", "a", "--root", str(c), "--no-fetch"])
        sh(c, "git", "add", "-A"); sh(c, "git", "commit", "-qm", "a")
        push_reservation(c, "res/y", "adr", 3)
        with mock.patch.object(N, "open_prs", return_value=OPEN):
            problems = CN.check(c, "origin/main", "mine")
        self.assertIn("ADR-0003 is also reserved on res/y: one of the two renumbers", problems)

    def test_a_reservation_whose_branch_was_deleted_or_whose_number_main_uses_holds_nothing(self):
        c = repo()
        push_reservation(c, "res/y", "adr", 1)  # main already uses ADR-0001
        push_reservation(c, "res/z", "adr", 9)
        sh(c, "git", "push", "-q", "origin", "--delete", "res/z")
        self.assertEqual(0, self.reserve(c))
        self.assertIn("| adr | 0003 | `mine` |", (c / "docs/NUMBERS.md").read_text())

    def test_a_stale_reservation_copied_into_the_registry_is_released_with_its_branch(self):
        c = repo()
        R.write_registry(c / "docs/NUMBERS.md", [
            {"kind": "adr", "number": 9, "branch": "res/z", "pr": "", "title": "gone", "state": "reserved"}])
        self.assertEqual(0, self.reserve(c))  # res/z is on no remote and in no local branch
        text = (c / "docs/NUMBERS.md").read_text()
        self.assertIn("| adr | 0003 | `mine` |", text)
        self.assertNotIn("res/z", text)

    def test_a_failed_fetch_reserves_nothing_and_says_why(self):
        c = repo()
        sh(c, "git", "remote", "set-url", "origin", str(c / "no-such-remote"))
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code = self.reserve(c)
        self.assertEqual(1, code)
        self.assertIn("cannot read the remote branches: git fetch origin failed", err.getvalue())
        self.assertIn("nothing was reserved", err.getvalue())
        self.assertFalse((c / "docs/NUMBERS.md").exists())

    def test_a_failed_fetch_fails_the_check_with_the_reason(self):
        c = repo()
        sh(c, "git", "remote", "set-url", "origin", str(c / "no-such-remote"))
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = CN.main(["--root", str(c), "--branch", "mine"])
        self.assertEqual(1, code)
        self.assertIn("numbers: FAIL: cannot read the remote branches", out.getvalue())

    def test_no_fetch_reserves_against_the_refs_already_here_and_says_so(self):
        c = repo()
        sh(c, "git", "remote", "set-url", "origin", str(c / "no-such-remote"))
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            self.assertEqual(0, self.reserve(c, "--no-fetch"))
        self.assertIn("--no-fetch, using the remote refs already here", err.getvalue())


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
