"""PUBLISH_SDKS publishes nothing under a registry name the operator has not chosen
(sdks/REGISTRY-NAMES.toml, scripts/check_sdk_registry_names.py), and the SDK file names come from
the manifests (scripts/sdk_names.py)."""
import io
import json
import re
import shutil
import sys
import tempfile
import tomllib
import unittest
from contextlib import redirect_stderr
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import check_sdk_registry_names as G  # noqa: E402
import sdk_names as N  # noqa: E402

REPO = N.DEFAULT_ROOT


def copy_of_the_repository(tmp: str, marker: str | None) -> Path:
    root = Path(tmp)
    for name in ("sdks/python/pyproject.toml", "sdks/typescript/package.json"):
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(REPO / name, root / name)
    if marker is not None:
        (root / G.MARKER).write_text(marker)
    return root


class RegistryNames(unittest.TestCase):
    def test_the_committed_marker_is_consistent_whichever_way_it_is_set(self):
        marker = tomllib.loads((REPO / G.MARKER).read_text())
        self.assertIn("decided", marker)
        if marker["decided"]:
            self.assertEqual(G.refusals(REPO), [])
        else:
            self.assertEqual(len(G.refusals(REPO)), 1)

    def test_while_the_names_are_undecided_the_guard_refuses_and_says_whose_choice_it_is(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = copy_of_the_repository(tmp, "decided = false\n")
            err = io.StringIO()
            with redirect_stderr(err):
                code = G.main(["--root", str(root)])
        self.assertEqual(code, 1)
        self.assertIn("decided is not true", err.getvalue())
        self.assertIn("operator", err.getvalue())

    def test_a_missing_marker_refuses(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = copy_of_the_repository(tmp, None)
            self.assertEqual(len(G.refusals(root)), 1)
            self.assertIn("does not exist", G.refusals(root)[0])

    def test_a_decision_that_is_not_the_manifests_names_is_refused_for_each_registry(self):
        python, npm = N.python_name(REPO), N.npm_name(REPO)
        with tempfile.TemporaryDirectory() as tmp:
            root = copy_of_the_repository(tmp, f'decided = true\npython = "{python}"\nnpm = "x-{npm}"\n')
            problems = G.refusals(root)
            self.assertEqual(len(problems), 1, problems)
            self.assertIn("npm is", problems[0])
            (root / G.MARKER).write_text(f'decided = true\npython = "x-{python}"\nnpm = "{npm}"\n')
            problems = G.refusals(root)
            self.assertEqual(len(problems), 1, problems)
            self.assertIn("python is", problems[0])
            (root / G.MARKER).write_text("decided = true\n")
            self.assertEqual(len(G.refusals(root)), 2)

    def test_a_decision_naming_the_manifests_names_passes(self):
        python, npm = N.python_name(REPO), N.npm_name(REPO)
        with tempfile.TemporaryDirectory() as tmp:
            root = copy_of_the_repository(tmp, f'decided = true\npython = "{python}"\nnpm = "{npm}"\n')
            self.assertEqual(G.refusals(root), [])

    def test_decided_must_be_the_boolean_true_not_a_string(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = copy_of_the_repository(tmp, 'decided = "true"\n')
            self.assertEqual(len(G.refusals(root)), 1)


class FileNames(unittest.TestCase):
    def test_the_file_names_are_the_ones_the_build_tools_write_for_the_manifests_names(self):
        self.assertEqual(N.wheel_filename(REPO, "1.2.3"), f"{N.python_stem(N.python_name(REPO))}-1.2.3-py3-none-any.whl")
        self.assertEqual(N.python_stem("Open.Memory_Lab-SDK"), "open_memory_lab_sdk")
        with tempfile.TemporaryDirectory() as tmp:
            root = copy_of_the_repository(tmp, None)
            manifest = root / "sdks/typescript/package.json"
            data = json.loads(manifest.read_text())
            data["name"] = "@open-memory-lab/sdk"
            manifest.write_text(json.dumps(data))
            self.assertEqual(N.tgz_filename(root, "1.2.3"), "open-memory-lab-sdk-1.2.3.tgz")

    def test_the_python_client_reads_its_own_version_under_the_manifests_name(self):
        source = (REPO / "sdks/python/src/oml_sdk/client.py").read_text()
        named = re.search(r"importlib\.metadata\.version\('([^']+)'\)", source)
        self.assertIsNotNone(named)
        self.assertEqual(named.group(1), N.python_name(REPO))

    def test_the_command_prints_the_three_names_for_the_workflow_outputs(self):
        out = io.StringIO()
        from contextlib import redirect_stdout

        with redirect_stdout(out):
            self.assertEqual(N.main(["--version", "1.2.3"]), 0)
        keys = [line.split("=", 1)[0] for line in out.getvalue().splitlines()]
        self.assertEqual(keys, ["sdk_wheel", "sdk_sdist", "sdk_tgz"])


if __name__ == "__main__":
    unittest.main()
