"""RELEASE-HOLD stops a release (scripts/check_release_hold.py), and release.yml runs that check
first in the job every other release job needs (scripts/check_release.py)."""
import copy
import io
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import yaml  # noqa: E402

import check_release as R  # noqa: E402
import check_release_hold as H  # noqa: E402


def run(root: Path) -> tuple[int, str]:
    err = io.StringIO()
    with redirect_stderr(err), redirect_stdout(io.StringIO()):
        code = H.main(["--root", str(root)])
    return code, err.getvalue()


class Hold(unittest.TestCase):
    def test_a_hold_file_refuses_the_release_naming_the_file_and_its_reason(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "RELEASE-HOLD").write_text("\nThe web console is mid-move.\nmore\n")
            code, err = run(Path(d))
        self.assertEqual(code, 1)
        self.assertIn("release held: RELEASE-HOLD exists", err)
        self.assertIn("The web console is mid-move.", err)

    def test_without_the_file_the_release_proceeds(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertEqual(run(Path(d)), (0, ""))

    def test_release_yml_runs_the_hold_first_and_every_job_needs_it(self):
        jobs = yaml.safe_load(R.WORKFLOW.read_text())["jobs"]
        errors: list[str] = []
        R.check_release_hold(errors, jobs)
        self.assertEqual(errors, [])

    def test_a_workflow_that_skips_the_hold_or_a_job_that_escapes_it_is_named(self):
        jobs = yaml.safe_load(R.WORKFLOW.read_text())["jobs"]
        skipped = copy.deepcopy(jobs)
        skipped["version"]["steps"] = [
            s for s in skipped["version"]["steps"] if "check_release_hold" not in str(s.get("run", ""))
        ]
        errors: list[str] = []
        R.check_release_hold(errors, skipped)
        self.assertEqual(errors, ["the version job does not run scripts/check_release_hold.py"])
        escaped = copy.deepcopy(jobs)
        escaped["cli"]["needs"] = []
        errors = []
        R.check_release_hold(errors, escaped)
        self.assertEqual(errors, ["job cli: does not need the version job, so RELEASE-HOLD would not stop it"])


if __name__ == "__main__":
    unittest.main()
