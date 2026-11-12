"""scripts/check_action_updates.py reads the pins the actions lint enforces and reports the ones that
trail the newest tag of their own major; a newer major is information only."""
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import check_action_updates as U  # noqa: E402

SHA = "a" * 40
OTHER = "b" * 40


class ActionUpdates(unittest.TestCase):
    def test_a_newer_tag_of_the_same_major_is_the_one_reported_and_floating_tags_are_ignored(self):
        tags = ["v4", "v4.3.0", "v4.4.0", "v4.10.1", "v4.9.9", "v5.0.0", "v3.9.9", "v4.4.0-rc1"]
        self.assertEqual(("v4.10.1", "v5.0.0"), U.newer("v4.4.0", tags))

    def test_an_action_at_the_newest_tag_of_its_major_has_nothing_to_report(self):
        self.assertEqual((None, None), U.newer("v4.10.1", ["v4.4.0", "v4.10.1"]))
        self.assertEqual((None, "v5.0.0"), U.newer("v4.10.1", ["v4.10.1", "v5.0.0"]))

    def test_the_report_fails_for_a_trailing_pin_and_not_for_a_newer_major_alone(self):
        pins = {"a/behind": (SHA, "v1.0.0"), "a/major": (SHA, "v1.0.0"), "a/current": (SHA, "v1.0.0")}
        tags = {"a/behind": ["v1.0.0", "v1.0.1"], "a/major": ["v1.0.0", "v2.0.0"], "a/current": ["v1.0.0"]}
        commit = lambda repo, tag: SHA if tag == "v1.0.0" else OTHER  # noqa: E731
        lines, behind = U.report(pins, tags, commit)
        self.assertTrue(behind)
        self.assertEqual(
            ["a/behind: pinned v1.0.0, newer v1.0.1", "a/major: pinned v1.0.0, newer major v2.0.0 (a reviewed change)"],
            lines,
        )
        self.assertEqual(([lines[1]], False), U.report({k: pins[k] for k in ("a/major",)}, tags, commit))

    def test_a_pin_that_is_not_the_commit_its_comment_names_is_a_finding(self):
        pins = {"a/moved": (SHA, "v1.0.0"), "a/gone": (SHA, "v9.9.9")}
        tags = {"a/moved": ["v1.0.0"], "a/gone": ["v1.0.0"]}
        commit = lambda repo, tag: {("a/moved", "v1.0.0"): OTHER}.get((repo, tag))  # noqa: E731
        lines, behind = U.report(pins, tags, commit)
        self.assertTrue(behind)
        self.assertEqual(
            [
                f"a/gone: pinned {SHA[:12]} names v9.9.9, which is no tag of a/gone",
                f"a/moved: pinned {SHA[:12]} is not the commit of v1.0.0 ({OTHER[:12]})",
            ],
            lines,
        )

    def test_a_pin_whose_comment_names_a_major_alone_is_checked_against_that_tag_and_the_newest_release(self):
        tags = {"a/b": ["v4", "v4.3.0", "v4.4.0", "v5.0.0"]}
        # The floating tag and the newest release of its major are both at the pin: nothing trails.
        at_newest = lambda repo, tag: SHA if tag in ("v4", "v4.4.0") else OTHER  # noqa: E731
        self.assertEqual(
            (["a/b: pinned v4, newer major v5.0.0 (a reviewed change)"], False),
            U.report({"a/b": (SHA, "v4")}, tags, at_newest),
        )
        # The floating tag moved on to a newer release: the pin is no longer it, and trails.
        moved = lambda repo, tag: SHA if tag == "v4.3.0" else OTHER  # noqa: E731
        lines, behind = U.report({"a/b": (SHA, "v4")}, tags, moved)
        self.assertTrue(behind)
        self.assertEqual(
            [
                f"a/b: pinned {SHA[:12]} is not the commit of v4 ({OTHER[:12]})",
                "a/b: pinned v4, newer v4.4.0",
                "a/b: pinned v4, newer major v5.0.0 (a reviewed change)",
            ],
            lines,
        )

    def test_the_pins_are_read_from_the_workflows_and_an_unpinned_line_is_not_one(self):
        with tempfile.TemporaryDirectory() as tmp:
            wf = Path(tmp) / ".github" / "workflows"
            wf.mkdir(parents=True)
            (wf / "a.yml").write_text(
                f"steps:\n  - uses: actions/checkout@{SHA} # v4.4.0\n  - uses: actions/cache@v4\n"
                f"  - uses: ./local\n    # uses: x/y@{SHA} # v1.0.0\n"
            )
            self.assertEqual({"actions/checkout": (SHA, "v4.4.0")}, U.pinned(Path(tmp)))

    def test_a_pin_is_read_however_the_lint_reads_it_and_a_sub_path_is_its_repository(self):
        with tempfile.TemporaryDirectory() as tmp:
            wf = Path(tmp) / ".github" / "workflows"
            wf.mkdir(parents=True)
            (wf / "a.yml").write_text(
                f"steps:\n  - {{ uses: actions/checkout@{SHA} }} # v4\n"
                f'  - "uses": actions/cache/restore@{OTHER} # v4.3\n'
                f"  - uses:\n      a/b@{SHA} # v1.2.3\n"
            )
            self.assertEqual(
                {"actions/checkout": (SHA, "v4"), "actions/cache": (OTHER, "v4.3"), "a/b": (SHA, "v1.2.3")},
                U.pinned(Path(tmp)),
            )

    def test_the_committed_workflows_yield_a_pin_for_every_action_they_use(self):
        root = Path(__file__).resolve().parents[2]
        # A sub-path of an action is its repository, and a repository's name has no case.
        used = {
            U.L.repository(line.split("uses:")[1].split("@")[0].strip())
            for f in (root / ".github").rglob("*.yml")
            for line in f.read_text().splitlines()
            if "uses:" in line and not line.strip().startswith("#")
        }
        self.assertEqual(used, set(U.pinned(root)))


if __name__ == "__main__":
    unittest.main()
