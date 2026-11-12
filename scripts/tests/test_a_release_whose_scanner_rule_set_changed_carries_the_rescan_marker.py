"""A release whose scanner rule set differs from the previous tag's carries the admin-rescan marker in
its notes (scripts/release_markers.py), and release.yml appends and checks it (scripts/check_release.py)."""
import copy
import io
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import yaml  # noqa: E402

import check_release as R  # noqa: E402
import release_markers as M  # noqa: E402

MARKER = "<!-- oml-deployer: post-step admin-rescan-record-held -->"


def source(ruleset):
    return f'/// The rule-set version.\npub const RULESET: &str = "{ruleset}";\n'


def repo(history):
    """A git repository with one commit and tag per (tag, ruleset) of `history`."""
    d = tempfile.TemporaryDirectory()
    root = Path(d.name)

    def git(*args):
        subprocess.run(["git", "-C", str(root), "-c", "user.name=t", "-c", "user.email=t@t", *args],
                       check=True, capture_output=True)

    git("init", "-q")
    path = root / M.SCANNER_SOURCE
    path.parent.mkdir(parents=True)
    for tag, ruleset in history:
        path.write_text(source(ruleset) if ruleset else "// no rule set here\n")
        git("add", "-A")
        git("commit", "-q", "--allow-empty", "-m", tag)
        git("tag", tag)
    return d, root


def run(root, *argv):
    out, err = io.StringIO(), io.StringIO()
    with redirect_stdout(out), redirect_stderr(err):
        code = M.main([*argv, "--root", str(root)])
    return code, out.getvalue(), err.getvalue()


class Markers(unittest.TestCase):
    def test_the_rule_set_is_read_from_the_constant(self):
        self.assertEqual(M.ruleset_of(source("oml-scan/1.2")), "oml-scan/1.2")
        self.assertIsNone(M.ruleset_of("// the rule set is oml-scan/1.2\n"))

    def test_the_previous_tag_is_the_greatest_stable_tag_below_it(self):
        tags = ["v0.7.0", "v0.7.2", "v0.7.10", "v0.6.9", "v0.8.0"]
        self.assertEqual(M.previous_tag("v0.7.10", tags), "v0.7.2")
        self.assertEqual(M.previous_tag("v0.7.2", tags), "v0.7.0")
        self.assertEqual(M.previous_tag("v0.8.0-rc1", tags), "v0.7.10")
        self.assertIsNone(M.previous_tag("v0.6.9", ["v0.7.0"]))
        with self.assertRaises(ValueError):
            M.previous_tag("main", tags)

    def test_a_changed_rule_set_requires_the_marker_and_notes_without_it_fail_naming_the_line(self):
        d, root = repo([("v0.7.1", "oml-scan/1.1"), ("v0.7.2", "oml-scan/1.2")])
        with d:
            notes = root / "notes.md"
            notes.write_text("## Install\n\nthe notes\n")
            code, _, err = run(root, "check", "--tag", "v0.7.2", str(notes))
            self.assertEqual(code, 1)
            self.assertIn(MARKER, err)
            self.assertIn("oml-scan/1.2 at v0.7.2 and oml-scan/1.1 at v0.7.1", err)
            self.assertIn("add it to the release notes source", err)
            self.assertEqual(run(root, "list", "--tag", "v0.7.2")[1].strip(), MARKER)

    def test_notes_that_carry_the_marker_pass_and_append_writes_it_once(self):
        d, root = repo([("v0.7.1", "oml-scan/1.1"), ("v0.7.2", "oml-scan/1.2")])
        with d:
            notes = root / "notes.md"
            notes.write_text("## Install\n\nthe notes\n")
            self.assertEqual(run(root, "append", "--tag", "v0.7.2", str(notes))[0], 0)
            self.assertEqual(run(root, "append", "--tag", "v0.7.2", str(notes))[0], 0)
            text = notes.read_text()
            self.assertEqual(text.count(MARKER), 1)
            self.assertTrue(text.startswith("## Install\n\nthe notes\n"))
            self.assertEqual(run(root, "check", "--tag", "v0.7.2", str(notes))[0], 0)

    def test_an_unchanged_rule_set_requires_no_marker(self):
        d, root = repo([("v0.7.0", "oml-scan/1.1"), ("v0.7.1", "oml-scan/1.1")])
        with d:
            notes = root / "notes.md"
            notes.write_text("the notes\n")
            self.assertEqual(run(root, "list", "--tag", "v0.7.1")[1], "")
            self.assertEqual(run(root, "check", "--tag", "v0.7.1", str(notes))[0], 0)
            self.assertEqual(run(root, "append", "--tag", "v0.7.1", str(notes))[0], 0)
            self.assertEqual(notes.read_text(), "the notes\n")

    def test_a_release_with_no_previous_tag_requires_no_marker(self):
        d, root = repo([("v0.1.0", "oml-scan/1")])
        with d:
            self.assertEqual(run(root, "list", "--tag", "v0.1.0"), (0, "", ""))

    def test_a_tag_whose_scanner_source_declares_no_rule_set_fails_by_name(self):
        d, root = repo([("v0.7.1", "oml-scan/1.1"), ("v0.7.2", "")])
        with d:
            code, _, err = run(root, "list", "--tag", "v0.7.2")
            self.assertEqual(code, 1)
            self.assertIn("declares no `pub const RULESET`", err)

    def test_a_previous_tag_that_declared_no_rule_set_counts_as_a_change(self):
        d, root = repo([("v0.7.1", ""), ("v0.7.2", "oml-scan/1.2")])
        with d:
            self.assertEqual(run(root, "list", "--tag", "v0.7.2")[1].strip(), MARKER)


class Workflow(unittest.TestCase):
    jobs = yaml.safe_load(R.WORKFLOW.read_text())["jobs"]

    @staticmethod
    def errors(jobs):
        found = []
        R.check_release_markers(found, jobs)
        return found

    def test_release_yml_appends_then_checks_the_markers_before_it_creates_the_release(self):
        self.assertEqual(self.errors(self.jobs), [])

    def test_a_release_job_that_does_not_check_the_markers_is_refused(self):
        jobs = copy.deepcopy(self.jobs)
        job = next(j for j in jobs.values() if R.step_index(j, "gh release create") is not None)
        for step in job["steps"]:
            if "release_markers.py" in str(step.get("run", "")):
                step["run"] = "echo skipped"
        got = self.errors(jobs)
        self.assertTrue(any("release_markers.py append" in e for e in got), got)
        self.assertTrue(any("release_markers.py check" in e for e in got), got)

    def test_a_release_job_that_checks_after_it_creates_the_release_is_refused(self):
        jobs = copy.deepcopy(self.jobs)
        job = next(j for j in jobs.values() if R.step_index(j, "gh release create") is not None)
        moved = [s for s in job["steps"] if "release_markers.py" in str(s.get("run", ""))]
        job["steps"] = [s for s in job["steps"] if s not in moved] + moved
        got = self.errors(jobs)
        self.assertTrue(any("before it creates the release" in e for e in got), got)

    def test_a_shallow_checkout_is_refused_since_it_holds_no_tags(self):
        jobs = copy.deepcopy(self.jobs)
        job = next(j for j in jobs.values() if R.step_index(j, "gh release create") is not None)
        for step in job["steps"]:
            if str(step.get("uses", "")).startswith("actions/checkout@"):
                step.pop("with", None)
        self.assertTrue(any("fetch-depth: 0" in e for e in self.errors(jobs)))


if __name__ == "__main__":
    unittest.main()
