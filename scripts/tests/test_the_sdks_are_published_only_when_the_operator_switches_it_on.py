"""release.yml builds and signs the SDK packages every release and uploads them to PyPI and npm
only from two jobs gated on PUBLISH_SDKS and on the operator's choice of registry names, each
checking every uploaded file by name against SHA256SUMS (scripts/check_release.py check_sdks)."""
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

    def test_npm_publish_of_a_bare_path_is_named_because_npm_reads_it_as_a_github_repository(self):
        for argument in ('"dist/$SDK_TGZ"', "dist/oml-sdk-1.tgz"):
            with self.subTest(argument):
                jobs = copy.deepcopy(JOBS)
                for step in jobs["publish-sdk-npm"]["steps"]:
                    if "npm publish" in str(step.get("run", "")):
                        step["run"] = f"npm publish {argument} --provenance --access public"
                self.assertEqual(
                    problems(jobs),
                    ["job publish-sdk-npm: npm publish does not start its argument with ./, so npm reads it as a GitHub repository"],
                )

    def test_a_checksum_step_that_passes_an_unlisted_file_is_named(self):
        for name in sorted(R.SDK_PUBLISHERS):
            with self.subTest(name):
                jobs = copy.deepcopy(JOBS)
                for step in jobs[name]["steps"]:
                    if "sha256sum --check --strict -" in str(step.get("run", "")):
                        step["run"] = "(cd dist && sha256sum --check --ignore-missing SHA256SUMS)"
                errors = problems(jobs)
                self.assertTrue(any("--ignore-missing" in e for e in errors), errors)
                self.assertTrue(any("sha256sum --check --strict -" in e for e in errors), errors)

    def test_a_publish_job_that_skips_naming_one_of_its_files_is_named(self):
        jobs = copy.deepcopy(JOBS)
        for step in jobs["publish-sdk-pypi"]["steps"]:
            step["run"] = str(step.get("run", "")).replace('"$SDK_SDIST"; do', '; do', 1)
        self.assertEqual(
            problems(jobs), ["job publish-sdk-pypi: does not check $SDK_SDIST by name against SHA256SUMS"]
        )

    def test_continue_on_error_or_or_true_on_a_publish_job_is_named(self):
        for name in sorted(R.SDK_PUBLISHERS):
            with self.subTest(name, how="job"):
                jobs = copy.deepcopy(JOBS)
                jobs[name]["continue-on-error"] = True
                self.assertEqual(
                    problems(jobs),
                    [f"job {name}: has continue-on-error, so a failed check would not stop the upload"],
                )
            with self.subTest(name, how="step"):
                jobs = copy.deepcopy(JOBS)
                jobs[name]["steps"][1]["continue-on-error"] = True
                self.assertEqual(
                    problems(jobs),
                    [f"job {name}: has continue-on-error, so a failed check would not stop the upload"],
                )
            with self.subTest(name, how="or true"):
                jobs = copy.deepcopy(JOBS)
                for step in jobs[name]["steps"]:
                    if "sha256sum" in str(step.get("run", "")):
                        step["run"] += "\nfalse || true\n"
                self.assertEqual(
                    problems(jobs), [f"job {name}: has a command ending in || true, which hides a failed check"]
                )

    def test_a_publish_job_that_loses_its_environment_is_named(self):
        for name, (environment, _) in sorted(R.SDK_PUBLISHERS.items()):
            with self.subTest(name):
                jobs = copy.deepcopy(JOBS)
                del jobs[name]["environment"]
                errors = problems(jobs)
                self.assertEqual(len(errors), 1, errors)
                self.assertIn(f"does not run in the {environment!r} environment", errors[0])

    def test_a_publish_job_that_does_not_stop_first_on_an_undecided_registry_name_is_named(self):
        for name in sorted(R.SDK_PUBLISHERS):
            with self.subTest(name, how="removed"):
                jobs = copy.deepcopy(JOBS)
                jobs[name]["steps"] = [
                    s for s in jobs[name]["steps"] if "check_sdk_registry_names" not in str(s.get("run", ""))
                ]
                self.assertEqual(
                    problems(jobs),
                    [f"job {name}: does not run scripts/check_sdk_registry_names.py, so an undecided registry name could publish"],
                )
            with self.subTest(name, how="after another command"):
                jobs = copy.deepcopy(JOBS)
                steps = jobs[name]["steps"]
                guard = next(i for i, s in enumerate(steps) if "check_sdk_registry_names" in str(s.get("run", "")))
                steps.insert(guard, {"run": "echo first"})
                self.assertEqual(
                    problems(jobs),
                    [f"job {name}: runs another command before scripts/check_sdk_registry_names.py"],
                )

    def test_an_action_pinned_by_tag_in_a_publish_job_is_named(self):
        jobs = copy.deepcopy(JOBS)
        jobs["publish-sdk-npm"]["steps"][0]["uses"] = "actions/download-artifact@v4"
        self.assertEqual(
            problems(jobs), ["job publish-sdk-npm: actions/download-artifact@v4 is not pinned by commit"]
        )

    def test_a_step_level_if_in_a_publish_job_is_named(self):
        for name in sorted(R.SDK_PUBLISHERS):
            for index in (1, 2):
                with self.subTest(name, step=index):
                    jobs = copy.deepcopy(JOBS)
                    jobs[name]["steps"][index]["if"] = "always()"
                    self.assertEqual(
                        problems(jobs),
                        [f"job {name}: has a step-level if:, which can skip a check and still reach the upload"],
                    )

    def test_a_publish_job_that_checks_the_files_after_it_uploads_them_is_named(self):
        for name in sorted(R.SDK_PUBLISHERS):
            with self.subTest(name):
                jobs = copy.deepcopy(JOBS)
                steps = jobs[name]["steps"]
                check = next(i for i, s in enumerate(steps) if "sha256sum --check --strict -" in str(s.get("run", "")))
                steps.append(steps.pop(check))
                self.assertEqual(problems(jobs), [f"job {name}: checks the files against SHA256SUMS after the upload"])

    def test_an_upload_written_with_flags_npx_or_pnpm_is_still_an_upload(self):
        for command in ("npm --tag next publish a.tgz", "npx npm publish a.tgz", "pnpm publish a.tgz", "yarn npm publish a.tgz"):
            with self.subTest(command, job="sdks"):
                jobs = copy.deepcopy(JOBS)
                jobs["sdks"]["steps"].append({"run": command})
                errors = problems(jobs)
                self.assertIn("job sdks: builds the SDK packages and uploads them to a registry", errors)
                self.assertTrue(any("only ['publish-sdk-npm', 'publish-sdk-pypi'] may" in e for e in errors), errors)
            with self.subTest(command, job="publish-sdk-npm"):
                jobs = copy.deepcopy(JOBS)
                for step in jobs["publish-sdk-npm"]["steps"]:
                    if "npm publish" in str(step.get("run", "")):
                        step["run"] = command
                self.assertEqual(
                    problems(jobs),
                    ["job publish-sdk-npm: npm publish does not start its argument with ./, so npm reads it as a GitHub repository"],
                )

    def test_an_npm_upload_written_with_flags_before_the_checksum_step_is_named(self):
        jobs = copy.deepcopy(JOBS)
        steps = jobs["publish-sdk-npm"]["steps"]
        steps.insert(2, {"run": 'npm --tag next publish "./dist/$SDK_TGZ"'})
        self.assertEqual(
            problems(jobs), ["job publish-sdk-npm: checks the files against SHA256SUMS after the upload"]
        )

    def test_the_pypi_action_with_another_directory_index_or_password_is_named(self):
        for key, value in (("packages-dir", "release"), ("repository-url", "https://example.org/legacy/"), ("password", "${{ secrets.X }}")):
            with self.subTest(key):
                jobs = copy.deepcopy(JOBS)
                action = next(s for s in jobs["publish-sdk-pypi"]["steps"] if "gh-action-pypi-publish" in str(s.get("uses", "")))
                action["with"][key] = value
                errors = problems(jobs)
                self.assertEqual(len(errors), 1, errors)
                self.assertTrue(errors[0].startswith(f"job publish-sdk-pypi: sets {key} on the PyPI action"), errors)

    def test_the_pypi_action_takes_print_hash_and_no_other_input(self):
        # The action reads `packages_dir` as it reads `packages-dir`, and `user`, `skip-existing`
        # and `attestations` each change what is uploaded, as whom, or what a rerun does.
        inputs = (
            ("packages_dir", "release"), ("repository_url", "https://example.org/legacy/"), ("user", "someone"),
            ("skip-existing", True), ("skip_existing", True), ("attestations", False), ("verbose", True),
        )
        for key, value in inputs:
            with self.subTest(key):
                jobs = copy.deepcopy(JOBS)
                action = next(s for s in jobs["publish-sdk-pypi"]["steps"] if "gh-action-pypi-publish" in str(s.get("uses", "")))
                action["with"][key] = value
                errors = problems(jobs)
                self.assertEqual(len(errors), 1, errors)
                self.assertTrue(errors[0].startswith(f"job publish-sdk-pypi: sets {key} on the PyPI action"), errors)
        with self.subTest("no with at all"):
            jobs = copy.deepcopy(JOBS)
            action = next(s for s in jobs["publish-sdk-pypi"]["steps"] if "gh-action-pypi-publish" in str(s.get("uses", "")))
            del action["with"]
            self.assertEqual(problems(jobs), [])

    def test_a_name_guard_that_cannot_stop_the_job_is_named(self):
        guard = "python3 scripts/check_sdk_registry_names.py"
        for written in (f"{guard} || echo undecided", f"{guard} &", f'echo "{guard}"', f"{guard}; true", f"if {guard}; then :; fi"):
            for name in sorted(R.SDK_PUBLISHERS):
                with self.subTest(written, job=name):
                    jobs = copy.deepcopy(JOBS)
                    for step in jobs[name]["steps"]:
                        if "check_sdk_registry_names" in str(step.get("run", "")):
                            step["run"] = written
                    self.assertEqual(
                        problems(jobs),
                        [f"job {name}: the registry-name step is not exactly `{guard}`, so its failure might not stop the job"],
                    )

    def test_a_checksum_command_that_cannot_stop_the_job_is_named(self):
        check = "printf '%s\\n' \"$line\" | sha256sum --check --strict -"
        for written in (f"{check} || echo mismatch", f"{check} &", f"echo \"{check}\"", f"{check} | cat"):
            for name in sorted(R.SDK_PUBLISHERS):
                with self.subTest(written, job=name):
                    jobs = copy.deepcopy(JOBS)
                    for step in jobs[name]["steps"]:
                        run = str(step.get("run", ""))
                        if check in run:
                            step["run"] = run.replace(check, written)
                    self.assertEqual(
                        problems(jobs),
                        [f"job {name}: a line that runs sha256sum --check --strict - is not exactly `{check}`, so a mismatch might not stop the job"],
                    )

    def test_anything_but_the_two_checked_files_reaching_dist_in_the_pypi_job_is_named(self):
        copy_line = 'cp "release/$SDK_WHEEL" "release/$SDK_SDIST" dist/'
        edits = {
            "a glob": lambda run: run.replace(copy_line, "cp release/* dist/"),
            "mkdir -p": lambda run: run.replace("mkdir dist", "mkdir -p dist"),
            "one more file": lambda run: run + "cp release/SHA256SUMS dist/\n",
            "a move into it": lambda run: run + 'mv "release/extra.whl" dist/\n',
        }
        for how, edit in edits.items():
            with self.subTest(how):
                jobs = copy.deepcopy(JOBS)
                for step in jobs["publish-sdk-pypi"]["steps"]:
                    if "sha256sum" in str(step.get("run", "")):
                        step["run"] = edit(step["run"])
                errors = problems(jobs)
                self.assertEqual(len(errors), 1, errors)
                self.assertIn("job publish-sdk-pypi: fills dist/ other than by", errors[0])
        with self.subTest("a later step"):
            jobs = copy.deepcopy(JOBS)
            steps = jobs["publish-sdk-pypi"]["steps"]
            upload = next(i for i, s in enumerate(steps) if "gh-action-pypi-publish" in str(s.get("uses", "")))
            steps.insert(upload, {"run": "curl -sSfo dist/extra.whl https://example.org/extra.whl"})
            errors = problems(jobs)
            self.assertIn("job publish-sdk-pypi: has a step between the checksum step and the upload, which can add files to dist/", errors)
        with self.subTest("a later step that names no dist"):
            jobs = copy.deepcopy(JOBS)
            steps = jobs["publish-sdk-pypi"]["steps"]
            upload = next(i for i, s in enumerate(steps) if "gh-action-pypi-publish" in str(s.get("uses", "")))
            steps.insert(upload, {"uses": "actions/download-artifact@" + "d" * 40, "with": {"path": "."}})
            self.assertEqual(
                problems(jobs),
                ["job publish-sdk-pypi: has a step between the checksum step and the upload, which can add files to dist/"],
            )

    def test_a_publish_job_that_checks_out_anything_but_the_tag_is_named(self):
        for name in sorted(R.SDK_PUBLISHERS):
            for ref in ("main", "${{ github.event.repository.default_branch }}", "refs/heads/main", "${{ github.sha }}"):
                with self.subTest(name, ref=ref):
                    jobs = copy.deepcopy(JOBS)
                    jobs[name]["steps"][0]["with"] = {"ref": ref}
                    self.assertEqual(
                        problems(jobs), [f"job {name}: checks out {ref!r}, not the tag that triggered the release"]
                    )
            for ref in ("${{ github.ref_name }}", "${{github.ref}}", "refs/tags/${{ github.ref_name }}"):
                with self.subTest(name, allowed=ref):
                    jobs = copy.deepcopy(JOBS)
                    jobs[name]["steps"][0]["with"] = {"ref": ref}
                    self.assertEqual(problems(jobs), [])

    def test_a_signing_job_that_drops_an_sdk_file_is_named(self):
        jobs = copy.deepcopy(JOBS)
        for step in jobs["binaries"]["steps"]:
            if "$SDK_TGZ" in str(step.get("run", "")):
                step["run"] = step["run"].replace("$SDK_TGZ", "")
        self.assertEqual(
            problems(jobs), ["job binaries: does not require $SDK_TGZ among the files it signs"]
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
