"""scripts/check_action_updates.py reads the pins the actions lint enforces and reports the ones that
trail the newest tag of their own major; a newer major is information only."""
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import check_action_updates as U  # noqa: E402

SHA = "a" * 40


class ActionUpdates(unittest.TestCase):
    def test_a_newer_tag_of_the_same_major_is_the_one_reported_and_floating_tags_are_ignored(self):
        tags = ["v4", "v4.3.0", "v4.4.0", "v4.10.1", "v4.9.9", "v5.0.0", "v3.9.9", "v4.4.0-rc1"]
        self.assertEqual(("v4.10.1", "v5.0.0"), U.newer("v4.4.0", tags))

    def test_an_action_at_the_newest_tag_of_its_major_has_nothing_to_report(self):
        self.assertEqual((None, None), U.newer("v4.10.1", ["v4.4.0", "v4.10.1"]))
        self.assertEqual((None, "v5.0.0"), U.newer("v4.10.1", ["v4.10.1", "v5.0.0"]))

    def test_the_report_fails_for_a_trailing_pin_and_not_for_a_newer_major_alone(self):
        pins = {"a/behind": "v1.0.0", "a/major": "v1.0.0", "a/current": "v1.0.0"}
        tags = {"a/behind": ["v1.0.0", "v1.0.1"], "a/major": ["v1.0.0", "v2.0.0"], "a/current": ["v1.0.0"]}
        lines, behind = U.report(pins, tags)
        self.assertTrue(behind)
        self.assertEqual(
            ["a/behind: pinned v1.0.0, newer v1.0.1", "a/major: pinned v1.0.0, newer major v2.0.0 (a reviewed change)"],
            lines,
        )
        self.assertEqual(([lines[1]], False), U.report({k: pins[k] for k in ("a/major",)}, tags))

    def test_the_pins_are_read_from_the_workflows_and_an_unpinned_line_is_not_one(self):
        with tempfile.TemporaryDirectory() as tmp:
            wf = Path(tmp) / ".github" / "workflows"
            wf.mkdir(parents=True)
            (wf / "a.yml").write_text(
                f"steps:\n  - uses: actions/checkout@{SHA} # v4.4.0\n  - uses: actions/cache@v4\n"
                f"  - uses: ./local\n    # uses: x/y@{SHA} # v1.0.0\n"
            )
            self.assertEqual({"actions/checkout": "v4.4.0"}, U.pinned(Path(tmp)))

    def test_the_committed_workflows_yield_a_pin_for_every_action_they_use(self):
        root = Path(__file__).resolve().parents[2]
        used = {
            line.split("uses:")[1].split("@")[0].strip()
            for f in (root / ".github").rglob("*.yml")
            for line in f.read_text().splitlines()
            if "uses:" in line and not line.strip().startswith("#")
        }
        self.assertEqual(used, set(U.pinned(root)))


if __name__ == "__main__":
    unittest.main()
