"""release.yml builds and signs the SDK packages every release and uploads them to PyPI and npm
only from two jobs gated on PUBLISH_SDKS (scripts/check_release.py check_sdks)."""
import copy
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import yaml  # noqa: E402

import check_release as R  # noqa: E402

JOBS = yaml.safe_load(R.WORKFLOW.read_text())["jobs"]


def problems(jobs: dict) -> list[str]:
    errors: list[str] = []
    R.check_sdks(errors, jobs)
    return errors


class SdkPublishing(unittest.TestCase):
    def test_the_workflow_as_committed_passes(self):
        self.assertEqual(problems(JOBS), [])

    def test_a_publish_job_without_the_switch_is_named(self):
        for name in sorted(R.SDK_PUBLISHERS):
            with self.subTest(name):
                jobs = copy.deepcopy(JOBS)
                del jobs[name]["if"]
                self.assertEqual(
                    problems(jobs),
                    [f"job {name}: is not gated on vars.PUBLISH_SDKS == 'true', so a release would publish by surprise"],
                )

    def test_a_publish_job_that_does_not_wait_for_the_signed_release_is_named(self):
        jobs = copy.deepcopy(JOBS)
        jobs["publish-sdk-pypi"]["needs"] = ["version", "sdks"]
        self.assertEqual(
            problems(jobs),
            ["job publish-sdk-pypi: does not need the version job and the job that creates the GitHub release"],
        )

    def test_an_upload_from_any_other_job_is_named(self):
        jobs = copy.deepcopy(JOBS)
        jobs["sdks"]["steps"].append({"run": "npm publish sdk-dist/typescript/oml-sdk-1.tgz"})
        errors = problems(jobs)
        self.assertIn("job sdks: builds the SDK packages and uploads them to a registry", errors)
        self.assertTrue(any("only ['publish-sdk-npm', 'publish-sdk-pypi'] may" in e for e in errors), errors)

    def test_an_action_pinned_by_tag_in_a_publish_job_is_named(self):
        jobs = copy.deepcopy(JOBS)
        jobs["publish-sdk-npm"]["steps"][0]["uses"] = "actions/download-artifact@v4"
        self.assertEqual(
            problems(jobs), ["job publish-sdk-npm: actions/download-artifact@v4 is not pinned by commit"]
        )

    def test_a_signing_job_that_drops_an_sdk_file_is_named(self):
        jobs = copy.deepcopy(JOBS)
        for step in jobs["binaries"]["steps"]:
            if "oml-sdk-${VERSION}.tgz" in str(step.get("run", "")):
                step["run"] = step["run"].replace("oml-sdk-${VERSION}.tgz", "")
        self.assertEqual(
            problems(jobs), ["job binaries: does not require oml-sdk-${VERSION}.tgz among the files it signs"]
        )

    def test_a_version_job_that_stops_checking_the_sdk_versions_is_named(self):
        jobs = copy.deepcopy(JOBS)
        for step in jobs["version"]["steps"]:
            step["run"] = str(step.get("run", "")).replace("check_sdk_versions", "x")
        self.assertEqual(
            problems(jobs), ["job version: does not run scripts/check_sdk_versions.py with the tag"]
        )


if __name__ == "__main__":
    unittest.main()
