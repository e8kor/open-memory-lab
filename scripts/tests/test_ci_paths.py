"""scripts/ci_paths.py: which CI jobs a change selects, and that the workflows use it as written."""
import re
import sys
import unittest
from pathlib import Path

import yaml

S = Path(__file__).resolve().parents[1]
ROOT = S.parent
sys.path.insert(0, str(S))
import ci_paths as P  # noqa: E402

WORKFLOWS = ROOT / ".github" / "workflows"
FILTERED = ["ci.yml", "docs-checks.yml", "m0-spikes.yml"]
# A required check that is a job of its own, not a `ci ok`: it reports on every pull
# request because the job always starts and only its steps are filtered (#331).
ALWAYS_REPORTING = {"ci.yml": {"web-walls": "crate walls (oml-web)"}}
# The filter each such job's steps carry: the output of the category the job guards.
ALWAYS_REPORTING_FILTER = {"web-walls": "needs.changes.outputs.cargo == 'true'"}


def on(files):
    return {k for k, v in P.categorize(files).items() if v}


class Selection(unittest.TestCase):
    def test_a_docs_only_change_selects_docs_and_the_docs_reading_tests_and_no_build(self):
        got = on(["docs/guides/first-steps.md", "changes/x.md", "mkdocs.yml"])
        self.assertEqual(got, {"docs", "docs_tests"})

    def test_a_page_a_rust_test_reads_selects_the_docs_reading_targets_not_the_workspace(self):
        for page in ["docs/architecture/roles-and-keys.md", "docs/reference/connect-page.md",
                     "docs/cli/reference.md", "docs/adr/ADR-0001-x.md"]:
            got = on([page])
            self.assertIn("docs_tests", got, page)
            self.assertNotIn("cargo", got, page)
            self.assertNotIn("rust", got, page)
        self.assertIn("cli_ref", on(["docs/cli/reference.md"]))
        self.assertNotIn("cli_ref", on(["docs/index.md"]))

    def test_an_eval_data_change_selects_the_targets_that_read_it_and_the_eval(self):
        self.assertEqual(on(["eval/recall/corpus.json"]), {"eval", "docs_tests"})

    def test_a_rust_crate_change_selects_the_build_and_nothing_about_docs(self):
        got = on(["crates/oml-scan/src/lib.rs"])
        self.assertEqual(got, {"rust", "cargo"})

    def test_a_migration_selects_the_build_and_the_migrations(self):
        self.assertEqual(on(["migrations/0042_x.sql"]), {"migrations", "cargo"})

    def test_the_openapi_document_is_a_page_and_a_compiled_input(self):
        got = on(["docs/api/openapi.yaml"])
        self.assertEqual(got, {"api", "docs", "cargo", "docs_tests"})
        self.assertIn("api", on(["docs/api/mcp-tools.json"]))
        self.assertNotIn("api", on(["docs/api/errors.md"]))

    def test_a_plugin_change_selects_the_plugin_and_not_the_build(self):
        self.assertEqual(on(["plugins/oml/skills/oml-memory/SKILL.md"]), {"plugins"})

    def test_an_sdk_change_selects_the_sdk_jobs(self):
        self.assertEqual(on(["sdks/python/pyproject.toml"]), {"sdk"})

    def test_a_rust_sdk_change_selects_the_cargo_build_as_well(self):
        # sdks/rust is the workspace member oml-sdk, which oml-web links.
        got = on(["sdks/rust/src/lib.rs"])
        self.assertTrue({"sdk", "rust", "cargo"} <= got, got)

    def test_a_deploy_change_selects_the_release_jobs(self):
        self.assertEqual(on(["deploy/helm/oml/values.yaml"]), {"release"})
        self.assertEqual(on(["Dockerfile"]), {"release"})

    def test_a_workflow_file_the_lockfile_the_toolchain_and_a_script_select_everything(self):
        for f in [".github/workflows/ci.yml", "Cargo.lock", "rust-toolchain.toml", "scripts/quickstart.sh",
                  "scripts/ci_paths.py", "scripts/tests/test_ci_paths.py"]:
            self.assertEqual(on([f]), set(P.OUTPUTS), f)

    def test_the_mkdocs_build_hook_selects_the_site_build(self):
        self.assertIn("docs", on(["hooks/status_line.py"]))
        self.assertNotIn("plugins", on(["hooks/status_line.py"]))

    def test_the_licenses_the_image_copies_select_the_release_jobs_and_the_site(self):
        self.assertEqual(on(["LICENSES/Apache-2.0.txt"]), {"docs", "docs_tests", "release"})

    def test_a_path_in_no_category_selects_everything(self):
        self.assertEqual(on(["docs/index.md", "some-new-dir/x"]), set(P.OUTPUTS))

    def test_a_mixed_change_is_the_union(self):
        got = on(["docs/index.md", "plugins/oml/README.md", "sdks/typescript/package.json"])
        self.assertEqual(got, {"docs", "docs_tests", "plugins", "sdk"})

    def test_an_empty_change_selects_nothing(self):
        self.assertEqual(on([]), set())


class Filtering(unittest.TestCase):
    docs = ["docs/index.md"]

    def test_only_a_pull_request_into_main_is_filtered(self):
        self.assertNotIn("cargo", on(self.docs))
        self.assertEqual(P.select("pull_request", "main", self.docs), P.categorize(self.docs))
        for event, base in [("push", ""), ("merge_group", ""), ("pull_request", "release/v0.6"), ("workflow_dispatch", "")]:
            self.assertEqual(P.select(event, base, self.docs), P.everything(), (event, base))

    def test_a_diff_that_cannot_be_computed_selects_everything(self):
        self.assertEqual(P.select("pull_request", "main", None), P.everything())


class Workflows(unittest.TestCase):
    """The workflows use the outputs this script emits, and the final job waits for every job."""

    @staticmethod
    def text(name):
        return (WORKFLOWS / name).read_text()

    @staticmethod
    def job_ids(text):
        return re.findall(r"^  ([a-z0-9-]+):$", text.split("\njobs:\n", 1)[1], re.M)

    def test_every_output_a_workflow_reads_is_one_the_script_emits(self):
        for wf in FILTERED:
            used = set(re.findall(r"needs\.changes\.outputs\.([a-z_]+)", self.text(wf)))
            self.assertTrue(used, wf)
            self.assertLessEqual(used, set(P.OUTPUTS), wf)
            declared = set(re.findall(r"^      ([a-z_]+): \$\{\{ steps\.paths\.outputs\.\1 \}\}$", self.text(wf), re.M))
            self.assertEqual(declared, set(P.OUTPUTS), wf)

    def test_every_job_of_a_filtered_workflow_waits_for_changes_or_is_the_final_check(self):
        for wf in FILTERED:
            t = self.text(wf)
            for job in self.job_ids(t):
                if job in ("changes", "ci-ok") or job in ALWAYS_REPORTING.get(wf, {}):
                    continue
                block = re.split(r"\n  [a-z0-9-]+:\n", t.split(f"\n  {job}:\n", 1)[1])[0]
                self.assertIn("needs: changes", block, f"{wf} {job}")
                self.assertRegex(block, r"\n    if: needs\.changes\.outputs\.", f"{wf} {job}")

    def test_a_required_check_that_is_a_job_of_its_own_always_starts_and_filters_its_steps(self):
        # A job skipped by a job-level `if` leaves no check run, so a required check of that
        # name waits for ever on a pull request that does not select it (#331). The job starts
        # on every pull request, under the same name, and each of its steps is the filtered part.
        for wf, jobs in ALWAYS_REPORTING.items():
            doc = yaml.safe_load(self.text(wf))
            for job, name in jobs.items():
                body = doc["jobs"][job]
                self.assertEqual(body["name"], name, f"{wf} {job}")
                self.assertEqual(body["needs"], "changes", f"{wf} {job}")
                self.assertNotIn("if", body, f"{wf} {job}: a job-level `if` leaves no check")
                self.assertTrue(body["steps"], f"{wf} {job}")
                self.assertEqual(self.steps_off_the_filter(body, ALWAYS_REPORTING_FILTER[job]), [], f"{wf} {job}")

    @staticmethod
    def steps_off_the_filter(body, guarded):
        """The steps of a job whose `if` is not exactly the filter the job guards."""
        return [step for step in body["steps"] if step.get("if") != guarded]

    def test_a_step_filtered_on_the_wrong_output_or_a_negated_or_forced_one_is_refused(self):
        guarded = ALWAYS_REPORTING_FILTER["web-walls"]
        body = yaml.safe_load(self.text("ci.yml"))["jobs"]["web-walls"]
        self.assertEqual(self.steps_off_the_filter(body, guarded), [])
        for wrong in ["needs.changes.outputs.docs == 'true'", "needs.changes.outputs.cargo != 'true'",
                      f"always() || {guarded}", f"{guarded} || always()", None]:
            moved = {"steps": [dict(step) for step in body["steps"]]}
            if wrong is None:
                moved["steps"][0].pop("if")
            else:
                moved["steps"][0]["if"] = wrong
            self.assertEqual(self.steps_off_the_filter(moved, guarded), [moved["steps"][0]], wrong)

    def test_every_required_check_is_a_ci_ok_or_a_job_that_always_starts(self):
        # The names the ruleset on main requires, which docs/contributing/ci-and-toolchain.md lists.
        required = {"ci ok (ci)", "ci ok (docs)", "ci ok (spikes)", "ci ok (practices)", "crate walls (oml-web)"}
        always = {name for jobs in ALWAYS_REPORTING.values() for name in jobs.values()}
        for name in required - always:
            self.assertTrue(name.startswith("ci ok ("), name)
        doc = (ROOT / "docs" / "contributing" / "ci-and-toolchain.md").read_text()
        for name in required:
            self.assertIn(f"| `{name}` |", doc, name)

    def test_ci_ok_needs_every_other_job_and_runs_always(self):
        for wf in [*FILTERED, "practices.yml"]:
            t = self.text(wf)
            ids = [j for j in self.job_ids(t) if j != "ci-ok"]
            block = t.split("\n  ci-ok:\n", 1)[1]
            self.assertIn("name: ci ok", block, wf)
            self.assertIn("if: always()", block, wf)
            needs = re.search(r"needs: \[(.*?)\]", block).group(1).split(", ")
            self.assertEqual(sorted(needs), sorted(ids), wf)

    def test_the_four_final_checks_have_four_distinct_names(self):
        names = []
        for wf in [*FILTERED, "practices.yml"]:
            block = self.text(wf).split("\n  ci-ok:\n", 1)[1]
            names.append(re.search(r"^    name: (.*)$", block, re.M).group(1))
        self.assertEqual(sorted(names), ["ci ok (ci)", "ci ok (docs)", "ci ok (practices)", "ci ok (spikes)"])

    def test_every_workflow_with_a_final_check_runs_in_a_merge_queue(self):
        # A required check that never reports in a merge group holds the queue forever.
        for wf in [*FILTERED, "practices.yml"]:
            on = self.text(wf).split("\non:\n", 1)[1].split("\n\n", 1)[0]
            self.assertRegex(on, r"(^|\n)  merge_group:", wf)

    def test_a_crashing_classifier_fails_the_changes_job_and_so_every_final_check(self):
        for wf in FILTERED:
            t = self.text(wf)
            step = t.split("      - id: paths\n", 1)[1].split("\n\n", 1)[0]
            self.assertIn("| tee", step, wf)
            self.assertRegex(step, r"(^|\n)        shell: bash\b", f"{wf}: a pipe needs -o pipefail")
            block = t.split("\n  ci-ok:\n", 1)[1]
            self.assertIn("changes", re.search(r"needs: \[(.*?)\]", block).group(1).split(", "), wf)

    def test_every_check_name_survives(self):
        old = {
            "ci.yml": ["fmt, clippy, doc", "cargo test (Postgres)", "demo and quickstart (release binaries)",
                       "release image (${{ matrix.platform }})", "oml CLI (${{ matrix.target }})", "cargo deny",
                       "agent plugin agrees with the server", "recall eval per search pipeline (served)",
                       "recall eval data parses and its labels resolve",
                       "kubernetes chart (kind, helm install, quickstart)",
                       "Python SDK against the release server", "TypeScript SDK against the release server",
                       "generated SDK code is current with openapi.yaml",
                       "crate walls (oml-web)"],
            "docs-checks.yml": ["mkdocs build --strict", "benchmark pages match their results",
                                "contract surfaces agree", "openapi.yaml validates",
                                "remember eval data and offline run",
                                "generated reference pages match their sources",
                                "CLI reference matches the oml binary's help"],
            "m0-spikes.yml": ["chain-format vectors reproduce outside Rust", "spike 3, RLS hardening (pg17)"],
        }
        for wf, names in old.items():
            t = self.text(wf)
            for n in names:
                self.assertIn(f"    name: {n}\n", t, f"{wf}: {n}")


if __name__ == "__main__":
    unittest.main()
